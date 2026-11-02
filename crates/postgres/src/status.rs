use ferristrace_application::repository;
use ferristrace_domain::{
    Accent, Announcement, Bulletin, ClaimedLetter, Component, ComponentId, ComponentName,
    ComponentStatus, DeliveryStatus, Email, IncidentStatus, IncidentTitle, Intro, Invite,
    LetterBody, LetterId, Link, Mode, PageName, ProjectId, RuleId, Saved, ServiceName, Settlement,
    Slug, StatusOutbox, StatusPage, StatusPageId, StatusPages, StatusSubscribers, StoreError,
    Subscriber, SubscriberId, TenantId, Theme, UpdateKind,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

const UNIQUE_VIOLATION: &str = "23505";
const PAGE_COLUMNS: &str = "id, org_id, name, slug, public, hide_from_search, accent, mode, \
                            intro, created_ms, updated_ms";
const COMPONENT_COLUMNS: &str = "id, page_id, name, link_kind, service, project_id, rule_id, \
                                 manual_status, position, public";
const SUBSCRIBER_COLUMNS: &str = "id, page_id, email, confirmed, invited_ms";

fn unreachable(doing: &'static str) -> impl FnOnce(sqlx::Error) -> StoreError {
    move |error| {
        tracing::error!(%error, "the control plane refused to {doing}");
        StoreError::Unreachable
    }
}

fn refused(what: &str) -> StoreError {
    tracing::error!(
        what,
        "a stored status page row carries a value the domain refuses"
    );
    StoreError::Unreachable
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn taken(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == UNIQUE_VIOLATION)
}

fn read_page(row: &PgRow) -> Result<StatusPage, StoreError> {
    Ok(StatusPage {
        id: StatusPageId::new(row.get("id")),
        org: TenantId::new(row.get("org_id")),
        name: PageName::try_from(row.get::<&str, _>("name")).map_err(|_| refused("name"))?,
        slug: Slug::try_from(row.get::<&str, _>("slug")).map_err(|_| refused("slug"))?,
        public: row.get("public"),
        hide_from_search: row.get("hide_from_search"),
        theme: Theme {
            accent: Accent::parse(row.get::<&str, _>("accent")).ok_or_else(|| refused("accent"))?,
            mode: Mode::parse(row.get::<&str, _>("mode")).ok_or_else(|| refused("mode"))?,
        },
        intro: row
            .get::<Option<&str>, _>("intro")
            .map(Intro::try_from)
            .transpose()
            .map_err(|_| refused("intro"))?,
        created_ms: unsigned(row.get("created_ms")),
        updated_ms: unsigned(row.get("updated_ms")),
    })
}

fn read_component(row: &PgRow) -> Result<Component, StoreError> {
    let link = match row.get::<&str, _>("link_kind") {
        "service" => Link::Service {
            name: row
                .get::<Option<&str>, _>("service")
                .map(ServiceName::try_from)
                .transpose()
                .map_err(|_| refused("service"))?
                .ok_or_else(|| refused("service"))?,
            project: row.get::<Option<Uuid>, _>("project_id").map(ProjectId::new),
        },
        "alert" => row
            .get::<Option<Uuid>, _>("rule_id")
            .map_or(Link::Manual, |rule| Link::Alert(RuleId::new(rule))),
        "manual" => Link::Manual,
        _ => return Err(refused("link_kind")),
    };
    Ok(Component {
        id: ComponentId::new(row.get("id")),
        page: StatusPageId::new(row.get("page_id")),
        name: ComponentName::try_from(row.get::<&str, _>("name")).map_err(|_| refused("name"))?,
        link,
        manual_status: row
            .get::<Option<&str>, _>("manual_status")
            .map(|held| ComponentStatus::parse(held).ok_or_else(|| refused("manual_status")))
            .transpose()?,
        position: u32::try_from(row.get::<i32, _>("position")).map_err(|_| refused("position"))?,
        public: row.get("public"),
    })
}

fn read_subscriber(row: &PgRow) -> Result<Subscriber, StoreError> {
    Ok(Subscriber {
        id: SubscriberId::new(row.get("id")),
        page: StatusPageId::new(row.get("page_id")),
        email: Email::try_from(row.get::<&str, _>("email")).map_err(|_| refused("email"))?,
        confirmed: row.get("confirmed"),
        invited_ms: unsigned(row.get("invited_ms")),
    })
}

struct LinkColumns<'a> {
    kind: &'static str,
    service: Option<&'a str>,
    project: Option<Uuid>,
    rule: Option<Uuid>,
}

fn link_columns(link: &Link) -> LinkColumns<'_> {
    match link {
        Link::Service { name, project } => LinkColumns {
            kind: "service",
            service: Some(name.as_str()),
            project: project.map(ProjectId::as_uuid),
            rule: None,
        },
        Link::Alert(rule) => LinkColumns {
            kind: "alert",
            service: None,
            project: None,
            rule: Some(rule.as_uuid()),
        },
        Link::Manual => LinkColumns {
            kind: "manual",
            service: None,
            project: None,
            rule: None,
        },
    }
}

#[repository(domain = StatusPages, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresStatusPages {
    tx: SharedTx,
}

impl PostgresStatusPages {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }

    async fn one(
        &self,
        id: StatusPageId,
        org: Option<TenantId>,
    ) -> Result<Option<StatusPage>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PAGE_COLUMNS} FROM status_pages \
             WHERE id = $1 AND ($2::uuid IS NULL OR org_id = $2)"
        )))
        .bind(id.as_uuid())
        .bind(org.map(TenantId::as_uuid))
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read a status page"))?;
        row.as_ref().map(read_page).transpose()
    }
}

impl StatusPages for PostgresStatusPages {
    async fn list(&self, org: TenantId) -> Result<Vec<StatusPage>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PAGE_COLUMNS} FROM status_pages WHERE org_id = $1 ORDER BY created_ms, id"
        )))
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the status pages"))?;
        rows.iter().map(read_page).collect()
    }

    async fn get(&self, org: TenantId, id: StatusPageId) -> Result<Option<StatusPage>, StoreError> {
        self.one(id, Some(org)).await
    }

    async fn by_slug(&self, slug: &Slug) -> Result<Option<StatusPage>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {PAGE_COLUMNS} FROM status_pages WHERE slug = $1"
        )))
        .bind(slug.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("find a status page by its slug"))?;
        row.as_ref().map(read_page).transpose()
    }

    async fn by_id(&self, id: StatusPageId) -> Result<Option<StatusPage>, StoreError> {
        self.one(id, None).await
    }

    async fn create(&self, page: &StatusPage) -> Result<Saved, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "INSERT INTO status_pages \
             (id, org_id, name, slug, public, hide_from_search, accent, mode, intro, \
              created_ms, updated_ms) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (slug) DO NOTHING",
        )
        .bind(page.id.as_uuid())
        .bind(page.org.as_uuid())
        .bind(page.name.as_str())
        .bind(page.slug.as_str())
        .bind(page.public)
        .bind(page.hide_from_search)
        .bind(page.theme.accent.as_str())
        .bind(page.theme.mode.as_str())
        .bind(page.intro.as_ref().map(Intro::as_str))
        .bind(signed(page.created_ms))
        .bind(signed(page.updated_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("create a status page"))?;
        Ok(if done.rows_affected() == 1 {
            Saved::Done
        } else {
            Saved::SlugTaken
        })
    }

    async fn keep(&self, page: &StatusPage) -> Result<Saved, StoreError> {
        let mut tx = self.tx.lock().await;
        let clashing: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM status_pages WHERE slug = $1 AND id <> $2)",
        )
        .bind(page.slug.as_str())
        .bind(page.id.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .map_err(unreachable("check a status page slug"))?;
        if clashing {
            return Ok(Saved::SlugTaken);
        }
        let done = sqlx::query(
            "UPDATE status_pages SET name = $3, slug = $4, public = $5, hide_from_search = $6, \
             accent = $7, mode = $8, intro = $9, updated_ms = $10 \
             WHERE org_id = $1 AND id = $2",
        )
        .bind(page.org.as_uuid())
        .bind(page.id.as_uuid())
        .bind(page.name.as_str())
        .bind(page.slug.as_str())
        .bind(page.public)
        .bind(page.hide_from_search)
        .bind(page.theme.accent.as_str())
        .bind(page.theme.mode.as_str())
        .bind(page.intro.as_ref().map(Intro::as_str))
        .bind(signed(page.updated_ms))
        .execute(&mut **tx)
        .await;
        match done {
            Ok(done) if done.rows_affected() == 1 => Ok(Saved::Done),
            Ok(_) => Ok(Saved::Gone),
            Err(error) if taken(&error) => Ok(Saved::SlugTaken),
            Err(error) => Err(unreachable("change a status page")(error)),
        }
    }

    async fn delete(&self, org: TenantId, id: StatusPageId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query("DELETE FROM status_pages WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(id.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(unreachable("delete a status page"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn components(
        &self,
        org: TenantId,
        page: StatusPageId,
    ) -> Result<Vec<Component>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COMPONENT_COLUMNS} FROM status_components \
             WHERE org_id = $1 AND page_id = $2 ORDER BY position, id"
        )))
        .bind(org.as_uuid())
        .bind(page.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the components of a status page"))?;
        rows.iter().map(read_component).collect()
    }

    async fn add_component(
        &self,
        org: TenantId,
        component: &Component,
    ) -> Result<bool, StoreError> {
        let columns = link_columns(&component.link);
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "INSERT INTO status_components \
             (id, org_id, page_id, name, link_kind, service, project_id, rule_id, \
              manual_status, position, public) \
             SELECT $1, p.org_id, p.id, $4, $5, $6, $7, $8, $9, $10, $11 \
             FROM status_pages p WHERE p.id = $3 AND p.org_id = $2 \
             AND ($7::uuid IS NULL OR EXISTS \
               (SELECT 1 FROM projects WHERE id = $7 AND org_id = $2)) \
             AND ($8::uuid IS NULL OR EXISTS \
               (SELECT 1 FROM alerts WHERE id = $8 AND org_id = $2))",
        )
        .bind(component.id.as_uuid())
        .bind(org.as_uuid())
        .bind(component.page.as_uuid())
        .bind(component.name.as_str())
        .bind(columns.kind)
        .bind(columns.service)
        .bind(columns.project)
        .bind(columns.rule)
        .bind(component.manual_status.map(ComponentStatus::as_str))
        .bind(i32::try_from(component.position).unwrap_or(i32::MAX))
        .bind(component.public)
        .execute(&mut **tx)
        .await
        .map_err(unreachable("add a status component"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn keep_component(
        &self,
        org: TenantId,
        component: &Component,
    ) -> Result<bool, StoreError> {
        let columns = link_columns(&component.link);
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE status_components SET name = $4, link_kind = $5, service = $6, \
             project_id = $7, rule_id = $8, manual_status = $9, position = $10, public = $11 \
             WHERE org_id = $1 AND page_id = $2 AND id = $3 \
             AND ($7::uuid IS NULL OR EXISTS \
               (SELECT 1 FROM projects WHERE id = $7 AND org_id = $1)) \
             AND ($8::uuid IS NULL OR EXISTS \
               (SELECT 1 FROM alerts WHERE id = $8 AND org_id = $1))",
        )
        .bind(org.as_uuid())
        .bind(component.page.as_uuid())
        .bind(component.id.as_uuid())
        .bind(component.name.as_str())
        .bind(columns.kind)
        .bind(columns.service)
        .bind(columns.project)
        .bind(columns.rule)
        .bind(component.manual_status.map(ComponentStatus::as_str))
        .bind(i32::try_from(component.position).unwrap_or(i32::MAX))
        .bind(component.public)
        .execute(&mut **tx)
        .await
        .map_err(unreachable("change a status component"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn remove_component(
        &self,
        org: TenantId,
        page: StatusPageId,
        id: ComponentId,
    ) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "DELETE FROM status_components WHERE org_id = $1 AND page_id = $2 AND id = $3",
        )
        .bind(org.as_uuid())
        .bind(page.as_uuid())
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(unreachable("remove a status component"))?;
        Ok(done.rows_affected() == 1)
    }
}

#[repository(domain = StatusSubscribers, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresStatusSubscribers {
    tx: SharedTx,
}

impl PostgresStatusSubscribers {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl StatusSubscribers for PostgresStatusSubscribers {
    async fn find(
        &self,
        page: StatusPageId,
        email: &Email,
    ) -> Result<Option<Subscriber>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {SUBSCRIBER_COLUMNS} FROM status_subscribers \
             WHERE page_id = $1 AND email = $2"
        )))
        .bind(page.as_uuid())
        .bind(email.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("find a status subscriber"))?;
        row.as_ref().map(read_subscriber).transpose()
    }

    async fn get(&self, id: SubscriberId) -> Result<Option<Subscriber>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {SUBSCRIBER_COLUMNS} FROM status_subscribers WHERE id = $1"
        )))
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read a status subscriber"))?;
        row.as_ref().map(read_subscriber).transpose()
    }

    async fn add(&self, subscriber: &Subscriber) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "INSERT INTO status_subscribers (id, page_id, email, confirmed, invited_ms) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT (page_id, email) DO NOTHING",
        )
        .bind(subscriber.id.as_uuid())
        .bind(subscriber.page.as_uuid())
        .bind(subscriber.email.as_str())
        .bind(subscriber.confirmed)
        .bind(signed(subscriber.invited_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("add a status subscriber"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn keep(&self, subscriber: &Subscriber) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE status_subscribers SET confirmed = $3, invited_ms = $4 \
             WHERE id = $1 AND page_id = $2",
        )
        .bind(subscriber.id.as_uuid())
        .bind(subscriber.page.as_uuid())
        .bind(subscriber.confirmed)
        .bind(signed(subscriber.invited_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("change a status subscriber"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn remove(&self, id: SubscriberId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query("DELETE FROM status_subscribers WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(unreachable("remove a status subscriber"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn confirmed(&self, page: StatusPageId) -> Result<u64, StoreError> {
        let mut tx = self.tx.lock().await;
        let counted: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM status_subscribers WHERE page_id = $1 AND confirmed",
        )
        .bind(page.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .map_err(unreachable("count the status subscribers"))?;
        Ok(unsigned(counted))
    }
}

#[repository(domain = StatusOutbox, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresStatusOutbox {
    tx: SharedTx,
}

impl PostgresStatusOutbox {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

fn read_letter(row: &PgRow) -> Result<ClaimedLetter, StoreError> {
    let body = match row.get::<&str, _>("kind") {
        "confirm" => LetterBody::Confirm,
        kind => LetterBody::Update(Bulletin {
            title: row
                .get::<Option<&str>, _>("title")
                .map(IncidentTitle::try_from)
                .transpose()
                .map_err(|_| refused("title"))?
                .ok_or_else(|| refused("title"))?,
            kind: UpdateKind::parse(kind).ok_or_else(|| refused("kind"))?,
            status: row
                .get::<Option<&str>, _>("incident_status")
                .and_then(IncidentStatus::parse)
                .ok_or_else(|| refused("incident_status"))?,
            text: row
                .get::<Option<&str>, _>("body")
                .ok_or_else(|| refused("body"))?
                .into(),
            at_ms: unsigned(row.get("at_ms")),
        }),
    };
    Ok(ClaimedLetter {
        id: LetterId::new(row.get("id")),
        attempt: u32::try_from(row.get::<i32, _>("attempt")).unwrap_or_default(),
        page: StatusPageId::new(row.get("page_id")),
        page_name: PageName::try_from(row.get::<&str, _>("page_name"))
            .map_err(|_| refused("name"))?,
        slug: Slug::try_from(row.get::<&str, _>("slug")).map_err(|_| refused("slug"))?,
        subscriber: SubscriberId::new(row.get("subscriber_id")),
        email: Email::try_from(row.get::<&str, _>("email")).map_err(|_| refused("email"))?,
        body,
    })
}

impl StatusOutbox for PostgresStatusOutbox {
    async fn announce(&self, announcement: &Announcement) -> Result<u64, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "INSERT INTO status_outbox \
             (id, page_id, subscriber_id, kind, incident_id, title, incident_status, body, \
              at_ms, next_at_ms) \
             SELECT gen_random_uuid(), s.page_id, s.id, $2, $3, $4, $5, $6, $7, $7 \
             FROM status_subscribers s JOIN status_pages p ON p.id = s.page_id \
             WHERE p.org_id = $1 AND p.public AND s.confirmed",
        )
        .bind(announcement.org.as_uuid())
        .bind(announcement.kind.as_str())
        .bind(announcement.incident.as_uuid())
        .bind(announcement.title.as_str())
        .bind(announcement.status.as_str())
        .bind(&*announcement.text)
        .bind(signed(announcement.at_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("announce to the status subscribers"))?;
        Ok(done.rows_affected())
    }

    async fn invite(&self, invite: &Invite) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO status_outbox (id, page_id, subscriber_id, kind, at_ms, next_at_ms) \
             SELECT $1, s.page_id, s.id, 'confirm', $4, $4 \
             FROM status_subscribers s WHERE s.id = $3 AND s.page_id = $2",
        )
        .bind(invite.id.as_uuid())
        .bind(invite.page.as_uuid())
        .bind(invite.subscriber.as_uuid())
        .bind(signed(invite.at_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("invite a status subscriber"))?;
        Ok(())
    }

    async fn claim(&self, now_ms: u64, limit: u32) -> Result<Vec<ClaimedLetter>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT o.id, o.attempt, o.kind, o.title, o.incident_status, o.body, o.at_ms, \
             p.id AS page_id, p.name AS page_name, p.slug, \
             s.id AS subscriber_id, s.email \
             FROM status_outbox o \
             JOIN status_pages p ON p.id = o.page_id \
             JOIN status_subscribers s ON s.id = o.subscriber_id \
             WHERE o.status = 'pending' AND o.next_at_ms <= $1 \
             ORDER BY o.next_at_ms, o.id LIMIT $2 \
             FOR UPDATE OF o SKIP LOCKED",
        )
        .bind(signed(now_ms))
        .bind(i64::from(limit))
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("claim the due status letters"))?;
        rows.iter().map(read_letter).collect()
    }

    async fn settle(
        &self,
        id: LetterId,
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
            "UPDATE status_outbox SET attempt = $2, status = $3, \
             next_at_ms = COALESCE($4, next_at_ms), last_error = $5 WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(i32::try_from(attempts).unwrap_or(i32::MAX))
        .bind(status.as_str())
        .bind(next_at_ms.map(signed))
        .bind(error)
        .execute(&mut **tx)
        .await
        .map_err(unreachable("settle a status letter"))?;
        Ok(())
    }
}
