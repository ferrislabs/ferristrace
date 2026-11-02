use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, AlertChannels, AlertDuty, AlertEvent, AlertEvents, AlertOutbox, AlertRules,
    AlertSeverity, AlertStates, AlertStatus, Channel, ChannelId, ChannelKind, ChannelName,
    ChannelView, Claimed, Clock, DeliveryStatus, Destination, Dispatch, Duty, EventId,
    Notification, NotificationId, ProjectId, Rule, RuleDraft, RuleId, RuleName, RuleState,
    SeriesState, Settlement, StoreError, TenantId, WebhookSecret,
};
use ferristrace_infra::SecretSeal;
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::alert_codec::{decode, encode, encode_labels, labels_of};
use crate::keys::to_nanos;
use crate::unit_of_work::{Postgres, SharedTx};

const NANOS_PER_MILLI: i64 = 1_000_000;
pub const EVALUATE_LOCK: i64 = 0x6674_616c_6572_7401;
pub const DELIVER_LOCK: i64 = 0x6674_616c_6572_7402;

fn unreachable(doing: &'static str) -> impl FnOnce(sqlx::Error) -> StoreError {
    move |error| {
        tracing::error!(%error, "the control plane refused to {doing}");
        StoreError::Unreachable
    }
}

fn refused(what: &str) -> StoreError {
    tracing::error!(
        what,
        "a stored alert row carries a value the domain refuses"
    );
    StoreError::Unreachable
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn read_rule(row: &PgRow) -> Result<Rule, StoreError> {
    let name: String = row.get("name");
    let definition: String = row.get("definition");
    Ok(Rule {
        id: RuleId::new(row.get("id")),
        org: TenantId::new(row.get("org_id")),
        name: RuleName::try_from(name.as_str()).map_err(|_| refused("name"))?,
        project: row.get::<Option<Uuid>, _>("project_id").map(ProjectId::new),
        enabled: row.get("enabled"),
        definition: decode(&definition).ok_or_else(|| refused("definition"))?,
        created_ms: unsigned(row.get::<i64, _>("created_at") / NANOS_PER_MILLI),
    })
}

#[repository(domain = AlertRules, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresAlertRules<C> {
    tx: SharedTx,
    clock: C,
}

impl<C: Clock + Clone> PostgresAlertRules<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            clock: backend.clock().clone(),
        }
    }
}

impl<C: Clock + Send + Sync> AlertRules for PostgresAlertRules<C> {
    async fn list(&self, org: TenantId) -> Result<Vec<Rule>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id, org_id, name, project_id, enabled, definition::text AS definition, created_at FROM alerts WHERE org_id = $1 ORDER BY created_at, id"
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the alert rules"))?;
        rows.iter().map(read_rule).collect()
    }

    async fn get(&self, org: TenantId, id: RuleId) -> Result<Option<Rule>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "SELECT id, org_id, name, project_id, enabled, definition::text AS definition, created_at FROM alerts WHERE org_id = $1 AND id = $2"
        )
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read an alert rule"))?;
        row.as_ref().map(read_rule).transpose()
    }

    async fn create(
        &self,
        org: TenantId,
        draft: &RuleDraft,
        by: AccountId,
    ) -> Result<Option<Rule>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "INSERT INTO alerts \
             (id, org_id, name, project_id, enabled, definition, created_by, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, $8, $8) \
             ON CONFLICT (org_id, name) DO NOTHING \
             RETURNING id, org_id, name, project_id, enabled, definition::text AS definition, created_at"
        )
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(draft.name.as_str())
        .bind(draft.project.map(ProjectId::as_uuid))
        .bind(draft.enabled)
        .bind(encode(&draft.definition))
        .bind(by.as_uuid())
        .bind(to_nanos(self.clock.now()))
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("create an alert rule"))?;
        row.as_ref().map(read_rule).transpose()
    }

    async fn update(&self, rule: &Rule) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE alerts SET name = $3, project_id = $4, enabled = $5, \
             definition = $6::jsonb, updated_at = $7 \
             WHERE org_id = $1 AND id = $2",
        )
        .bind(rule.org.as_uuid())
        .bind(rule.id.as_uuid())
        .bind(rule.name.as_str())
        .bind(rule.project.map(ProjectId::as_uuid))
        .bind(rule.enabled)
        .bind(encode(&rule.definition))
        .bind(to_nanos(self.clock.now()))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("update an alert rule"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn delete(&self, org: TenantId, id: RuleId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query("DELETE FROM alerts WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(id.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(unreachable("delete an alert rule"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn enabled(&self) -> Result<Vec<Rule>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id, org_id, name, project_id, enabled, definition::text AS definition, created_at FROM alerts WHERE enabled ORDER BY org_id, created_at, id"
        )
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the enabled alert rules"))?;
        Ok(rows.iter().filter_map(|row| read_rule(row).ok()).collect())
    }
}

fn status_of(row: &PgRow, column: &str) -> Result<AlertStatus, StoreError> {
    AlertStatus::parse(row.get::<&str, _>(column)).ok_or_else(|| refused(column))
}

fn read_series(row: &PgRow) -> Result<SeriesState, StoreError> {
    Ok(SeriesState {
        labels: labels_of(row.get::<&str, _>("labels")).ok_or_else(|| refused("labels"))?,
        state: RuleState {
            status: status_of(row, "status")?,
            value: row.get("value"),
            since_ms: unsigned(row.get("since_ms")),
            evaluated_ms: unsigned(row.get("evaluated_ms")),
        },
    })
}

#[repository(domain = AlertStates, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresAlertStates {
    tx: SharedTx,
}

impl PostgresAlertStates {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl AlertStates for PostgresAlertStates {
    async fn of(&self, rule: RuleId) -> Result<Vec<SeriesState>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT labels::text AS labels, status, value, since_ms, evaluated_ms \
             FROM alert_states WHERE rule_id = $1 ORDER BY labels_key",
        )
        .bind(rule.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("read the states of a rule"))?;
        rows.iter().map(read_series).collect()
    }

    async fn of_org(&self, org: TenantId) -> Result<Vec<(RuleId, SeriesState)>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT s.rule_id AS rule_id, s.labels::text AS labels, s.status AS status, \
             s.value AS value, s.since_ms AS since_ms, s.evaluated_ms AS evaluated_ms \
             FROM alert_states s JOIN alerts a ON a.id = s.rule_id \
             WHERE a.org_id = $1 ORDER BY s.rule_id, s.labels_key",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("read the states of an organisation"))?;
        rows.iter()
            .map(|row| Ok((RuleId::new(row.get("rule_id")), read_series(row)?)))
            .collect()
    }

    async fn keep(&self, rule: RuleId, state: &SeriesState) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO alert_states \
             (rule_id, labels_key, labels, status, value, since_ms, evaluated_ms) \
             VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7) \
             ON CONFLICT (rule_id, labels_key) DO UPDATE SET \
             labels = EXCLUDED.labels, status = EXCLUDED.status, value = EXCLUDED.value, \
             since_ms = EXCLUDED.since_ms, evaluated_ms = EXCLUDED.evaluated_ms",
        )
        .bind(rule.as_uuid())
        .bind(state.labels.key())
        .bind(encode_labels(&state.labels))
        .bind(state.state.status.as_str())
        .bind(state.state.value)
        .bind(signed(state.state.since_ms))
        .bind(signed(state.state.evaluated_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("keep the state of a rule"))?;
        Ok(())
    }
}

#[repository(domain = AlertEvents, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresAlertEvents {
    tx: SharedTx,
}

impl PostgresAlertEvents {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl AlertEvents for PostgresAlertEvents {
    async fn record(&self, event: &AlertEvent) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO alert_events \
             (id, rule_id, org_id, labels, from_status, to_status, value, at_ms) \
             VALUES ($1, $2, $3, $4::jsonb, $5, $6, $7, $8)",
        )
        .bind(event.id.as_uuid())
        .bind(event.rule.as_uuid())
        .bind(event.org.as_uuid())
        .bind(encode_labels(&event.labels))
        .bind(event.from.as_str())
        .bind(event.to.as_str())
        .bind(event.value)
        .bind(signed(event.at_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("record an alert event"))?;
        Ok(())
    }

    async fn history(
        &self,
        org: TenantId,
        rule: RuleId,
        from_ms: u64,
        to_ms: u64,
        limit: u32,
    ) -> Result<Vec<AlertEvent>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id, labels::text AS labels, from_status, to_status, value, at_ms \
             FROM alert_events \
             WHERE org_id = $1 AND rule_id = $2 AND at_ms >= $3 AND at_ms <= $4 \
             ORDER BY at_ms DESC, id DESC LIMIT $5",
        )
        .bind(org.as_uuid())
        .bind(rule.as_uuid())
        .bind(signed(from_ms))
        .bind(signed(to_ms))
        .bind(i64::from(limit))
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("read the history of a rule"))?;
        rows.iter()
            .map(|row| {
                Ok(AlertEvent {
                    id: EventId::new(row.get("id")),
                    rule,
                    org,
                    labels: labels_of(row.get::<&str, _>("labels"))
                        .ok_or_else(|| refused("labels"))?,
                    from: status_of(row, "from_status")?,
                    to: status_of(row, "to_status")?,
                    value: row.get("value"),
                    at_ms: unsigned(row.get("at_ms")),
                })
            })
            .collect()
    }

    async fn prune(&self, before_ms: u64) -> Result<u64, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query("DELETE FROM alert_events WHERE at_ms < $1")
            .bind(signed(before_ms))
            .execute(&mut **tx)
            .await
            .map_err(unreachable("forget the old alert events"))?;
        Ok(done.rows_affected())
    }
}

fn open_secret(seal: Option<&SecretSeal>, row: &PgRow) -> Option<WebhookSecret> {
    let sealed: Option<Vec<u8>> = row.get("secret_sealed");
    let opened = seal?.unseal_bytes(&sealed?);
    if opened.is_none() {
        tracing::error!("a webhook secret does not open with the configured key at rest");
    }
    let opened = opened?;
    core::str::from_utf8(&opened)
        .ok()
        .map(WebhookSecret::from_plaintext)
}

fn destination_of(row: &PgRow, kind: &str, target: &str) -> Result<Destination, StoreError> {
    let kind = ChannelKind::parse(row.get::<&str, _>(kind)).ok_or_else(|| refused("kind"))?;
    Destination::parse(kind, row.get::<&str, _>(target)).map_err(|_| refused("target"))
}

fn channel_name(row: &PgRow, column: &str) -> Result<ChannelName, StoreError> {
    ChannelName::try_from(row.get::<&str, _>(column)).map_err(|_| refused("channel name"))
}

#[repository(domain = AlertOutbox, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresAlertOutbox {
    tx: SharedTx,
    seal: Option<SecretSeal>,
}

impl PostgresAlertOutbox {
    pub fn new<C>(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            seal: backend.seal.clone(),
        }
    }

    fn claimed(&self, row: &PgRow) -> Result<Claimed, StoreError> {
        let rule = RuleId::new(row.get("rule_id"));
        let org = TenantId::new(row.get("org_id"));
        let channel = ChannelId::new(row.get("channel_id"));
        let at_ms = unsigned(row.get("at_ms"));
        let from = status_of(row, "from_status")?;
        let to = status_of(row, "to_status")?;
        let value = row.get("value");
        Ok(Claimed {
            notification: Notification {
                id: NotificationId::new(row.get("id")),
                org,
                rule,
                event: EventId::new(row.get("event_id")),
                channel,
                attempt: u32::try_from(row.get::<i32, _>("attempt")).unwrap_or_default(),
                next_at_ms: unsigned(row.get("next_at_ms")),
                status: DeliveryStatus::Pending,
                last_error: row.get::<Option<String>, _>("last_error").map(Into::into),
            },
            dispatch: Dispatch {
                rule,
                name: row.get::<&str, _>("rule_name").into(),
                severity: AlertSeverity::parse(row.get::<&str, _>("severity"))
                    .ok_or_else(|| refused("severity"))?,
                org,
                from,
                to,
                value,
                labels: labels_of(row.get::<&str, _>("labels")).ok_or_else(|| refused("labels"))?,
                at_ms,
            },
            channel: Channel {
                id: channel,
                name: channel_name(row, "channel_name")?,
                destination: destination_of(row, "kind", "target")?,
                secret: open_secret(self.seal.as_ref(), row),
            },
        })
    }
}

impl AlertOutbox for PostgresAlertOutbox {
    async fn enqueue(&self, notification: &Notification) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO alert_outbox \
             (id, org_id, rule_id, event_id, channel_id, attempt, next_at_ms, status, last_error) \
             SELECT $1, $2, $3, $4, c.id, $6, $7, $8, $9 \
             FROM alert_channels c WHERE c.id = $5 AND c.org_id = $2 \
             ON CONFLICT (event_id, channel_id) DO NOTHING",
        )
        .bind(notification.id.as_uuid())
        .bind(notification.org.as_uuid())
        .bind(notification.rule.as_uuid())
        .bind(notification.event.as_uuid())
        .bind(notification.channel.as_uuid())
        .bind(i32::try_from(notification.attempt).unwrap_or(i32::MAX))
        .bind(signed(notification.next_at_ms))
        .bind(notification.status.as_str())
        .bind(notification.last_error.as_deref())
        .execute(&mut **tx)
        .await
        .map_err(unreachable("queue a notification"))?;
        Ok(())
    }

    async fn claim(&self, now_ms: u64, limit: u32) -> Result<Vec<Claimed>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT o.id AS id, o.org_id AS org_id, o.rule_id AS rule_id, \
             o.event_id AS event_id, o.channel_id AS channel_id, o.attempt AS attempt, \
             o.next_at_ms AS next_at_ms, o.last_error AS last_error, \
             a.name AS rule_name, a.definition->>'severity' AS severity, \
             e.labels::text AS labels, e.from_status AS from_status, e.to_status AS to_status, \
             e.value AS value, e.at_ms AS at_ms, \
             c.name AS channel_name, c.kind AS kind, c.target AS target, \
             c.secret_sealed AS secret_sealed \
             FROM alert_outbox o \
             JOIN alerts a ON a.id = o.rule_id \
             JOIN alert_events e ON e.id = o.event_id \
             JOIN alert_channels c ON c.id = o.channel_id \
             WHERE o.status = 'pending' AND o.next_at_ms <= $1 \
             ORDER BY o.next_at_ms, o.id \
             LIMIT $2 \
             FOR UPDATE OF o SKIP LOCKED",
        )
        .bind(signed(now_ms))
        .bind(i64::from(limit))
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("claim the due notifications"))?;
        rows.iter().map(|row| self.claimed(row)).collect()
    }

    async fn settle(
        &self,
        id: NotificationId,
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
            "UPDATE alert_outbox SET attempt = $2, status = $3, \
             next_at_ms = COALESCE($4, next_at_ms), last_error = $5 WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(i32::try_from(attempts).unwrap_or(i32::MAX))
        .bind(status.as_str())
        .bind(next_at_ms.map(signed))
        .bind(error)
        .execute(&mut **tx)
        .await
        .map_err(unreachable("settle a notification"))?;
        Ok(())
    }
}

#[repository(domain = AlertChannels, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresAlertChannels<C> {
    tx: SharedTx,
    clock: C,
    seal: Option<SecretSeal>,
}

impl<C: Clock + Clone> PostgresAlertChannels<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            clock: backend.clock().clone(),
            seal: backend.seal.clone(),
        }
    }
}

fn read_view(row: &PgRow) -> Result<ChannelView, StoreError> {
    Ok(ChannelView {
        id: ChannelId::new(row.get("id")),
        name: channel_name(row, "name")?,
        destination: destination_of(row, "kind", "target")?,
        has_secret: row.get("has_secret"),
    })
}

impl<C: Clock + Send + Sync> AlertChannels for PostgresAlertChannels<C> {
    async fn list(&self, org: TenantId) -> Result<Vec<ChannelView>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id, name, kind, target, secret_sealed IS NOT NULL AS has_secret \
             FROM alert_channels WHERE org_id = $1 ORDER BY created_at, id",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the alert channels"))?;
        rows.iter().map(read_view).collect()
    }

    async fn armed(&self, org: TenantId, id: ChannelId) -> Result<Option<Channel>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "SELECT id, name, kind, target, secret_sealed \
             FROM alert_channels WHERE org_id = $1 AND id = $2",
        )
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read an alert channel"))?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(Channel {
            id,
            name: channel_name(&row, "name")?,
            destination: destination_of(&row, "kind", "target")?,
            secret: open_secret(self.seal.as_ref(), &row),
        }))
    }

    async fn create(
        &self,
        org: TenantId,
        name: &ChannelName,
        destination: &Destination,
        secret: Option<&WebhookSecret>,
        by: AccountId,
    ) -> Result<Option<ChannelView>, StoreError> {
        let sealed = match secret {
            None => None,
            Some(secret) => Some(
                self.seal
                    .as_ref()
                    .and_then(|seal| seal.seal_bytes(secret.exposed().as_bytes()))
                    .ok_or_else(|| {
                        tracing::error!(
                            "a webhook secret was to be kept but secrets.at_rest_key is not configured"
                        );
                        StoreError::Unreachable
                    })?,
            ),
        };
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "INSERT INTO alert_channels \
             (id, org_id, name, kind, target, secret_sealed, created_by, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (org_id, name) DO NOTHING \
             RETURNING id, name, kind, target, secret_sealed IS NOT NULL AS has_secret",
        )
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(name.as_str())
        .bind(destination.kind().as_str())
        .bind(destination.target())
        .bind(sealed)
        .bind(by.as_uuid())
        .bind(to_nanos(self.clock.now()))
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("create an alert channel"))?;
        row.as_ref().map(read_view).transpose()
    }

    async fn update(
        &self,
        org: TenantId,
        id: ChannelId,
        name: &ChannelName,
        destination: &Destination,
    ) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE alert_channels SET name = $3, target = $4 \
             WHERE org_id = $1 AND id = $2 AND kind = $5",
        )
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .bind(name.as_str())
        .bind(destination.target())
        .bind(destination.kind().as_str())
        .execute(&mut **tx)
        .await
        .map_err(unreachable("update an alert channel"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn delete(&self, org: TenantId, id: ChannelId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "UPDATE alerts SET definition = jsonb_set(definition, '{channels}', \
             (definition->'channels') - $2::text) \
             WHERE org_id = $1 AND definition->'channels' ? $2::text",
        )
        .bind(org.as_uuid())
        .bind(id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(unreachable("unlink a channel from its rules"))?;
        let done = sqlx::query("DELETE FROM alert_channels WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(id.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(unreachable("delete an alert channel"))?;
        Ok(done.rows_affected() == 1)
    }

    fn keeps_secrets(&self) -> bool {
        self.seal.is_some()
    }
}

#[repository(domain = AlertDuty, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresAlertDuty {
    tx: SharedTx,
}

impl PostgresAlertDuty {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

fn lock_of(duty: Duty) -> i64 {
    match duty {
        Duty::Evaluate => EVALUATE_LOCK,
        Duty::Deliver => DELIVER_LOCK,
    }
}

impl AlertDuty for PostgresAlertDuty {
    async fn lead(&self, duty: Duty, slot: u64) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let held: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
            .bind(lock_of(duty))
            .fetch_one(&mut **tx)
            .await
            .map_err(unreachable("take the alert lock"))?;
        if !held {
            return Ok(false);
        }
        let done = sqlx::query(
            "INSERT INTO alert_duties (duty, slot) VALUES ($1, $2) \
             ON CONFLICT (duty) DO UPDATE SET slot = EXCLUDED.slot \
             WHERE alert_duties.slot < EXCLUDED.slot",
        )
        .bind(duty.as_str())
        .bind(signed(slot))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("claim an alert slot"))?;
        Ok(done.rows_affected() == 1)
    }
}
