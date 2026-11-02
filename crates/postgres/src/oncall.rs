use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, ClaimedNotice, Day, DeliveryStatus, Email, EventId, Handoff, IncidentId, Notice,
    NoticeId, Origin, Override, OverrideId, Page, PageDispatch, PageId, PageScope, PageSource,
    PageState, Pages, Rotation, RotationLength, RuleId, Schedule, ScheduleDraft, ScheduleId,
    ScheduleName, Schedules, Settlement, StoreError, TeamId, TenantId, TimeOfDay, Timezone,
    Weekday,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

const LISTED_PAGES: i64 = 200;
const SCHEDULE_COLUMNS: &str = "s.id AS id, s.org_id AS org_id, s.team_id AS team_id, \
     s.name AS name, s.timezone AS timezone, s.handoff_weekday AS handoff_weekday, \
     s.handoff_minute AS handoff_minute, s.length_days AS length_days, \
     s.starts_on::text AS starts_on, \
     COALESCE((SELECT array_agg(r.account_id ORDER BY r.position) FROM oncall_rotation r \
     WHERE r.schedule_id = s.id), '{}') AS rotation";
const PAGE_COLUMNS: &str = "p.id AS id, p.org_id AS org_id, p.schedule_id AS schedule_id, \
     p.target_id AS target_id, p.rule_id AS rule_id, p.event_id AS event_id, \
     p.incident_id AS incident_id, p.headline AS headline, p.state AS state, p.step AS step, \
     p.created_ms AS created_ms, p.escalate_at_ms AS escalate_at_ms, \
     p.acknowledged_by AS acknowledged_by, p.acknowledged_ms AS acknowledged_ms, \
     p.resolved_ms AS resolved_ms";

fn unreachable(doing: &'static str) -> impl FnOnce(sqlx::Error) -> StoreError {
    move |error| {
        tracing::error!(%error, "the control plane refused to {doing}");
        StoreError::Unreachable
    }
}

fn refused(what: &str) -> StoreError {
    tracing::error!(
        what,
        "a stored on-call row carries a value the domain refuses"
    );
    StoreError::Unreachable
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn small(value: u16) -> i16 {
    i16::try_from(value).unwrap_or(i16::MAX)
}

fn read_schedule(row: &PgRow) -> Result<Schedule, StoreError> {
    let weekday = u8::try_from(row.get::<i16, _>("handoff_weekday"))
        .ok()
        .and_then(Weekday::from_monday)
        .ok_or_else(|| refused("weekday"))?;
    let minute = u16::try_from(row.get::<i16, _>("handoff_minute"))
        .ok()
        .and_then(|minute| TimeOfDay::from_minutes(minute).ok())
        .ok_or_else(|| refused("handoff"))?;
    let rotation: Vec<Uuid> = row.get("rotation");
    Ok(Schedule {
        id: ScheduleId::new(row.get("id")),
        org: TenantId::new(row.get("org_id")),
        team: TeamId::new(row.get("team_id")),
        name: ScheduleName::try_from(row.get::<&str, _>("name")).map_err(|_| refused("name"))?,
        timezone: Timezone::try_from(row.get::<&str, _>("timezone"))
            .map_err(|_| refused("timezone"))?,
        rotation: Rotation::new(rotation.into_iter().map(AccountId::new).collect())
            .map_err(|_| refused("rotation"))?,
        handoff: Handoff {
            weekday,
            time: minute,
        },
        length: u64::try_from(row.get::<i16, _>("length_days"))
            .ok()
            .and_then(|days| RotationLength::try_from(days).ok())
            .ok_or_else(|| refused("length"))?,
        starts_on: Day::try_from(row.get::<&str, _>("starts_on"))
            .map_err(|_| refused("starts_on"))?,
    })
}

fn read_override(row: &PgRow) -> Override {
    Override {
        id: OverrideId::new(row.get("id")),
        schedule: ScheduleId::new(row.get("schedule_id")),
        account: AccountId::new(row.get("account_id")),
        from_ms: unsigned(row.get("from_ms")),
        to_ms: unsigned(row.get("to_ms")),
    }
}

#[repository(domain = Schedules, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresSchedules {
    tx: SharedTx,
}

impl PostgresSchedules {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }

    async fn seat(&self, schedule: ScheduleId, rotation: &Rotation) -> Result<(), StoreError> {
        let people: Vec<Uuid> = rotation
            .people()
            .iter()
            .map(|person| person.as_uuid())
            .collect();
        let mut tx = self.tx.lock().await;
        sqlx::query("DELETE FROM oncall_rotation WHERE schedule_id = $1")
            .bind(schedule.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(unreachable("clear a rotation"))?;
        sqlx::query(
            "INSERT INTO oncall_rotation (schedule_id, position, account_id) \
             SELECT $1, (seat.position - 1)::integer, seat.account \
             FROM unnest($2::uuid[]) WITH ORDINALITY AS seat(account, position)",
        )
        .bind(schedule.as_uuid())
        .bind(people)
        .execute(&mut **tx)
        .await
        .map_err(unreachable("keep a rotation"))?;
        Ok(())
    }
}

impl Schedules for PostgresSchedules {
    async fn of_team(&self, org: TenantId, team: TeamId) -> Result<Vec<Schedule>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {SCHEDULE_COLUMNS} FROM oncall_schedules s \
             WHERE s.org_id = $1 AND s.team_id = $2 ORDER BY s.created_at, s.id"
        )))
        .bind(org.as_uuid())
        .bind(team.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the schedules of a team"))?;
        rows.iter().map(read_schedule).collect()
    }

    async fn of_org(&self, org: TenantId) -> Result<Vec<Schedule>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {SCHEDULE_COLUMNS} FROM oncall_schedules s \
             WHERE s.org_id = $1 ORDER BY s.created_at, s.id"
        )))
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the schedules of an organisation"))?;
        rows.iter().map(read_schedule).collect()
    }

    async fn get(&self, org: TenantId, id: ScheduleId) -> Result<Option<Schedule>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {SCHEDULE_COLUMNS} FROM oncall_schedules s WHERE s.org_id = $1 AND s.id = $2"
        )))
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read a schedule"))?;
        row.as_ref().map(read_schedule).transpose()
    }

    async fn create(
        &self,
        org: TenantId,
        team: TeamId,
        draft: &ScheduleDraft,
    ) -> Result<Option<Schedule>, StoreError> {
        let id = ScheduleId::fresh();
        let made: Option<Uuid> = {
            let mut tx = self.tx.lock().await;
            sqlx::query_scalar(
                "INSERT INTO oncall_schedules \
                 (id, org_id, team_id, name, timezone, handoff_weekday, handoff_minute, \
                  length_days, starts_on) \
                 SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9::date \
                 WHERE EXISTS (SELECT 1 FROM teams WHERE org_id = $2 AND id = $3) \
                 ON CONFLICT (org_id, team_id, name) DO NOTHING RETURNING id",
            )
            .bind(id.as_uuid())
            .bind(org.as_uuid())
            .bind(team.as_uuid())
            .bind(draft.name.as_str())
            .bind(draft.timezone.name())
            .bind(i16::from(draft.handoff.weekday.days_from_monday()))
            .bind(small(draft.handoff.time.minutes()))
            .bind(i16::from(draft.length.days()))
            .bind(draft.starts_on.to_string())
            .fetch_optional(&mut **tx)
            .await
            .map_err(unreachable("create a schedule"))?
        };
        if made.is_none() {
            return Ok(None);
        }
        self.seat(id, &draft.rotation).await?;
        self.get(org, id).await
    }

    async fn update(&self, schedule: &Schedule) -> Result<bool, StoreError> {
        let done = {
            let mut tx = self.tx.lock().await;
            sqlx::query(
                "UPDATE oncall_schedules SET name = $3, timezone = $4, handoff_weekday = $5, \
                 handoff_minute = $6, length_days = $7, starts_on = $8::date \
                 WHERE org_id = $1 AND id = $2",
            )
            .bind(schedule.org.as_uuid())
            .bind(schedule.id.as_uuid())
            .bind(schedule.name.as_str())
            .bind(schedule.timezone.name())
            .bind(i16::from(schedule.handoff.weekday.days_from_monday()))
            .bind(small(schedule.handoff.time.minutes()))
            .bind(i16::from(schedule.length.days()))
            .bind(schedule.starts_on.to_string())
            .execute(&mut **tx)
            .await
            .map_err(unreachable("update a schedule"))?
        };
        if done.rows_affected() != 1 {
            return Ok(false);
        }
        self.seat(schedule.id, &schedule.rotation).await?;
        Ok(true)
    }

    async fn delete(&self, org: TenantId, id: ScheduleId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "UPDATE alerts SET definition = jsonb_set(definition, '{page}', 'null'::jsonb) \
             WHERE org_id = $1 AND definition->'page'->>'schedule' = $2",
        )
        .bind(org.as_uuid())
        .bind(id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(unreachable("unlink a schedule from its rules"))?;
        let done = sqlx::query("DELETE FROM oncall_schedules WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(id.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(unreachable("delete a schedule"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn overrides(
        &self,
        org: TenantId,
        schedule: ScheduleId,
    ) -> Result<Vec<Override>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id, schedule_id, account_id, from_ms, to_ms FROM oncall_overrides \
             WHERE org_id = $1 AND schedule_id = $2 ORDER BY from_ms, id",
        )
        .bind(org.as_uuid())
        .bind(schedule.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the overrides of a schedule"))?;
        Ok(rows.iter().map(read_override).collect())
    }

    async fn add_override(&self, org: TenantId, cover: &Override) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO oncall_overrides (id, org_id, schedule_id, account_id, from_ms, to_ms) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(cover.id.as_uuid())
        .bind(org.as_uuid())
        .bind(cover.schedule.as_uuid())
        .bind(cover.account.as_uuid())
        .bind(signed(cover.from_ms))
        .bind(signed(cover.to_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("keep an override"))?;
        Ok(())
    }

    async fn remove_override(
        &self,
        org: TenantId,
        schedule: ScheduleId,
        id: OverrideId,
    ) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "DELETE FROM oncall_overrides WHERE org_id = $1 AND schedule_id = $2 AND id = $3",
        )
        .bind(org.as_uuid())
        .bind(schedule.as_uuid())
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(unreachable("remove an override"))?;
        Ok(done.rows_affected() == 1)
    }
}

fn source_of(row: &PgRow) -> Result<PageSource, StoreError> {
    let rule: Option<Uuid> = row.get("rule_id");
    let event: Option<Uuid> = row.get("event_id");
    let incident: Option<Uuid> = row.get("incident_id");
    match (rule, event, incident) {
        (Some(rule), Some(event), None) => Ok(PageSource::Alert {
            rule: RuleId::new(rule),
            event: EventId::new(event),
        }),
        (None, None, Some(incident)) => Ok(PageSource::Incident(IncidentId::new(incident))),
        _ => Err(refused("source")),
    }
}

fn read_page(row: &PgRow) -> Result<Page, StoreError> {
    Ok(Page {
        id: PageId::new(row.get("id")),
        org: TenantId::new(row.get("org_id")),
        schedule: ScheduleId::new(row.get("schedule_id")),
        target: AccountId::new(row.get("target_id")),
        source: source_of(row)?,
        headline: row.get::<&str, _>("headline").into(),
        state: PageState::parse(row.get::<&str, _>("state")).ok_or_else(|| refused("state"))?,
        step: u32::try_from(row.get::<i32, _>("step")).map_err(|_| refused("step"))?,
        created_ms: unsigned(row.get("created_ms")),
        escalate_at_ms: row.get::<Option<i64>, _>("escalate_at_ms").map(unsigned),
        acknowledged_by: row
            .get::<Option<Uuid>, _>("acknowledged_by")
            .map(AccountId::new),
        acknowledged_ms: row.get::<Option<i64>, _>("acknowledged_ms").map(unsigned),
        resolved_ms: row.get::<Option<i64>, _>("resolved_ms").map(unsigned),
    })
}

fn source_columns(source: PageSource) -> (Option<Uuid>, Option<Uuid>, Option<Uuid>) {
    match source {
        PageSource::Alert { rule, event } => (Some(rule.as_uuid()), Some(event.as_uuid()), None),
        PageSource::Incident(incident) => (None, None, Some(incident.as_uuid())),
    }
}

fn step_of(step: u32) -> i32 {
    i32::try_from(step).unwrap_or(i32::MAX)
}

#[repository(domain = Pages, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresPages {
    tx: SharedTx,
}

impl PostgresPages {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl Pages for PostgresPages {
    async fn raise(&self, page: &Page) -> Result<bool, StoreError> {
        let (rule, event, incident) = source_columns(page.source);
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "INSERT INTO pages \
             (id, org_id, schedule_id, target_id, rule_id, event_id, incident_id, headline, \
              state, step, created_ms, escalate_at_ms) \
             SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12 \
             WHERE EXISTS (SELECT 1 FROM oncall_schedules WHERE org_id = $2 AND id = $3) \
             AND ($5::uuid IS NULL OR EXISTS (SELECT 1 FROM alerts WHERE org_id = $2 AND id = $5)) \
             AND ($7::uuid IS NULL OR EXISTS (SELECT 1 FROM incidents WHERE org_id = $2 AND id = $7)) \
             ON CONFLICT DO NOTHING",
        )
        .bind(page.id.as_uuid())
        .bind(page.org.as_uuid())
        .bind(page.schedule.as_uuid())
        .bind(page.target.as_uuid())
        .bind(rule)
        .bind(event)
        .bind(incident)
        .bind(&*page.headline)
        .bind(page.state.as_str())
        .bind(step_of(page.step))
        .bind(signed(page.created_ms))
        .bind(page.escalate_at_ms.map(signed))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("raise a page"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn get(&self, org: TenantId, id: PageId) -> Result<Option<Page>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PAGE_COLUMNS} FROM pages p WHERE p.org_id = $1 AND p.id = $2"
        )))
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read a page"))?;
        row.as_ref().map(read_page).transpose()
    }

    async fn list(&self, org: TenantId, scope: PageScope) -> Result<Vec<Page>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PAGE_COLUMNS} FROM pages p \
             WHERE p.org_id = $1 AND ($2 OR p.state <> 'resolved') \
             ORDER BY p.created_ms DESC, p.id LIMIT $3"
        )))
        .bind(org.as_uuid())
        .bind(scope == PageScope::All)
        .bind(LISTED_PAGES)
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the pages"))?;
        rows.iter().map(read_page).collect()
    }

    async fn due(&self, now_ms: u64, limit: u32) -> Result<Vec<Page>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PAGE_COLUMNS} FROM pages p \
             WHERE p.state IN ('triggered', 'escalated') AND p.escalate_at_ms <= $1 \
             ORDER BY p.escalate_at_ms, p.id LIMIT $2 FOR UPDATE OF p SKIP LOCKED"
        )))
        .bind(signed(now_ms))
        .bind(i64::from(limit))
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("claim the pages due to escalate"))?;
        rows.iter().map(read_page).collect()
    }

    async fn keep(&self, page: &Page) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE pages SET target_id = $3, state = $4, step = $5, escalate_at_ms = $6, \
             acknowledged_by = $7, acknowledged_ms = $8, resolved_ms = $9 \
             WHERE org_id = $1 AND id = $2",
        )
        .bind(page.org.as_uuid())
        .bind(page.id.as_uuid())
        .bind(page.target.as_uuid())
        .bind(page.state.as_str())
        .bind(step_of(page.step))
        .bind(page.escalate_at_ms.map(signed))
        .bind(page.acknowledged_by.map(AccountId::as_uuid))
        .bind(page.acknowledged_ms.map(signed))
        .bind(page.resolved_ms.map(signed))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("keep a page"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn resolve(
        &self,
        org: TenantId,
        origin: Origin,
        at_ms: u64,
    ) -> Result<Vec<Page>, StoreError> {
        let (column, id) = match origin {
            Origin::Rule(rule) => ("rule_id", rule.as_uuid()),
            Origin::Incident(incident) => ("incident_id", incident.as_uuid()),
        };
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE pages p SET state = 'resolved', escalate_at_ms = NULL, resolved_ms = $3 \
             WHERE p.org_id = $1 AND p.{column} = $2 AND p.state <> 'resolved' \
             RETURNING {PAGE_COLUMNS}"
        )))
        .bind(org.as_uuid())
        .bind(id)
        .bind(signed(at_ms))
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("resolve pages"))?;
        rows.iter().map(read_page).collect()
    }

    async fn enqueue(&self, notice: &Notice) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO page_outbox \
             (id, org_id, page_id, step, account_id, attempt, next_at_ms, status, last_error) \
             SELECT $1, p.org_id, p.id, $4, $5, $6, $7, $8, $9 \
             FROM pages p WHERE p.id = $3 AND p.org_id = $2 \
             ON CONFLICT (page_id, step, account_id) DO NOTHING",
        )
        .bind(notice.id.as_uuid())
        .bind(notice.org.as_uuid())
        .bind(notice.page.as_uuid())
        .bind(step_of(notice.step))
        .bind(notice.to.as_uuid())
        .bind(i32::try_from(notice.attempt).unwrap_or(i32::MAX))
        .bind(signed(notice.next_at_ms))
        .bind(notice.status.as_str())
        .bind(notice.last_error.as_deref())
        .execute(&mut **tx)
        .await
        .map_err(unreachable("queue a page notice"))?;
        Ok(())
    }

    async fn claim(&self, now_ms: u64, limit: u32) -> Result<Vec<ClaimedNotice>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PAGE_COLUMNS}, o.id AS notice_id, o.step AS notice_step, \
             o.account_id AS notice_to, o.attempt AS attempt, o.next_at_ms AS next_at_ms, \
             o.last_error AS last_error, a.email AS email \
             FROM page_outbox o \
             JOIN pages p ON p.id = o.page_id \
             JOIN accounts a ON a.id = o.account_id \
             WHERE o.status = 'pending' AND o.next_at_ms <= $1 \
             ORDER BY o.next_at_ms, o.id LIMIT $2 \
             FOR UPDATE OF o SKIP LOCKED"
        )))
        .bind(signed(now_ms))
        .bind(i64::from(limit))
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("claim the due page notices"))?;
        rows.iter()
            .map(|row| {
                let page = read_page(row)?;
                let step =
                    u32::try_from(row.get::<i32, _>("notice_step")).map_err(|_| refused("step"))?;
                Ok(ClaimedNotice {
                    notice: Notice {
                        id: NoticeId::new(row.get("notice_id")),
                        org: page.org,
                        page: page.id,
                        step,
                        to: AccountId::new(row.get("notice_to")),
                        attempt: u32::try_from(row.get::<i32, _>("attempt")).unwrap_or_default(),
                        next_at_ms: unsigned(row.get("next_at_ms")),
                        status: DeliveryStatus::Pending,
                        last_error: row.get::<Option<String>, _>("last_error").map(Into::into),
                    },
                    email: Email::try_from(row.get::<&str, _>("email"))
                        .map_err(|_| refused("email"))?,
                    dispatch: PageDispatch {
                        page: page.id,
                        org: page.org,
                        source: page.source,
                        headline: page.headline,
                        step,
                        at_ms: page.created_ms,
                        ack: None,
                    },
                })
            })
            .collect()
    }

    async fn settle(
        &self,
        id: NoticeId,
        attempts: u32,
        settlement: &Settlement,
    ) -> Result<(), StoreError> {
        let (status, next_at_ms, error) = match settlement {
            Settlement::Sent => (DeliveryStatus::Sent, None, None),
            Settlement::Retry { next_at_ms, error } => {
                (DeliveryStatus::Pending, Some(*next_at_ms), Some(&**error))
            }
            Settlement::Abandoned { error } => (DeliveryStatus::Abandoned, None, Some(&**error)),
        };
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "UPDATE page_outbox SET attempt = $2, status = $3, \
             next_at_ms = COALESCE($4, next_at_ms), last_error = $5 WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(i32::try_from(attempts).unwrap_or(i32::MAX))
        .bind(status.as_str())
        .bind(next_at_ms.map(signed))
        .bind(error)
        .execute(&mut **tx)
        .await
        .map_err(unreachable("settle a page notice"))?;
        Ok(())
    }
}
