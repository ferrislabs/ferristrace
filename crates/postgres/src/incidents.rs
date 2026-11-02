use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, Attachment, AttachmentId, AttachmentKind, AttachmentLabel, Incident,
    IncidentAttachments, IncidentFilter, IncidentId, IncidentScope, IncidentSeverity,
    IncidentStatus, IncidentTimeline, IncidentTitle, Incidents, ProjectId, Reference, RuleId,
    StoreError, TenantId, TimelineEntry, TimelineId,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::incident_codec::{author_of, author_parts, decode_change, encode_change};
use crate::unit_of_work::{Postgres, SharedTx};

fn unreachable(doing: &'static str) -> impl FnOnce(sqlx::Error) -> StoreError {
    move |error| {
        tracing::error!(%error, "the control plane refused to {doing}");
        StoreError::Unreachable
    }
}

fn refused(what: &str) -> StoreError {
    tracing::error!(
        what,
        "a stored incident row carries a value the domain refuses"
    );
    StoreError::Unreachable
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn read_incident(row: &PgRow) -> Result<Incident, StoreError> {
    Ok(Incident {
        id: IncidentId::new(row.get("id")),
        org: TenantId::new(row.get("org_id")),
        project: row.get::<Option<Uuid>, _>("project_id").map(ProjectId::new),
        title: IncidentTitle::try_from(row.get::<&str, _>("title"))
            .map_err(|_| refused("title"))?,
        severity: IncidentSeverity::parse(row.get::<&str, _>("severity"))
            .ok_or_else(|| refused("severity"))?,
        status: IncidentStatus::parse(row.get::<&str, _>("status"))
            .ok_or_else(|| refused("status"))?,
        owner: row.get::<Option<Uuid>, _>("owner_id").map(AccountId::new),
        public: row.get("public"),
        opened_ms: unsigned(row.get("opened_ms")),
        resolved_ms: row.get::<Option<i64>, _>("resolved_ms").map(unsigned),
        opened_from_alert: row
            .get::<Option<Uuid>, _>("opened_from_alert")
            .map(RuleId::new),
    })
}

#[repository(domain = Incidents, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresIncidents {
    tx: SharedTx,
}

impl PostgresIncidents {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

fn listing(scope: IncidentScope) -> &'static str {
    match scope {
        IncidentScope::Open => {
            "SELECT id, org_id, project_id, title, severity, status, owner_id, public, \
             opened_ms, resolved_ms, opened_from_alert FROM incidents \
             WHERE org_id = $1 AND ($2::uuid IS NULL OR project_id = $2) \
             AND status <> 'resolved' ORDER BY opened_ms DESC, id"
        }
        IncidentScope::Resolved => {
            "SELECT id, org_id, project_id, title, severity, status, owner_id, public, \
             opened_ms, resolved_ms, opened_from_alert FROM incidents \
             WHERE org_id = $1 AND ($2::uuid IS NULL OR project_id = $2) \
             AND status = 'resolved' ORDER BY opened_ms DESC, id"
        }
        IncidentScope::All => {
            "SELECT id, org_id, project_id, title, severity, status, owner_id, public, \
             opened_ms, resolved_ms, opened_from_alert FROM incidents \
             WHERE org_id = $1 AND ($2::uuid IS NULL OR project_id = $2) \
             ORDER BY opened_ms DESC, id"
        }
    }
}

impl Incidents for PostgresIncidents {
    async fn list(
        &self,
        org: TenantId,
        filter: &IncidentFilter,
    ) -> Result<Vec<Incident>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(listing(filter.scope))
            .bind(org.as_uuid())
            .bind(filter.project.map(ProjectId::as_uuid))
            .fetch_all(&mut **tx)
            .await
            .map_err(unreachable("list the incidents"))?;
        rows.iter().map(read_incident).collect()
    }

    async fn get(&self, org: TenantId, id: IncidentId) -> Result<Option<Incident>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "SELECT id, org_id, project_id, title, severity, status, owner_id, public, \
             opened_ms, resolved_ms, opened_from_alert FROM incidents \
             WHERE org_id = $1 AND id = $2",
        )
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read an incident"))?;
        row.as_ref().map(read_incident).transpose()
    }

    async fn open(&self, incident: &Incident) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO incidents \
             (id, org_id, project_id, title, severity, status, owner_id, public, \
              opened_ms, resolved_ms, opened_from_alert) \
             SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11 \
             WHERE $11::uuid IS NULL OR EXISTS \
               (SELECT 1 FROM alerts WHERE id = $11 AND org_id = $2)",
        )
        .bind(incident.id.as_uuid())
        .bind(incident.org.as_uuid())
        .bind(incident.project.map(ProjectId::as_uuid))
        .bind(incident.title.as_str())
        .bind(incident.severity.as_str())
        .bind(incident.status.as_str())
        .bind(incident.owner.map(AccountId::as_uuid))
        .bind(incident.public)
        .bind(signed(incident.opened_ms))
        .bind(incident.resolved_ms.map(signed))
        .bind(incident.opened_from_alert.map(RuleId::as_uuid))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("open an incident"))?;
        Ok(())
    }

    async fn keep(&self, incident: &Incident) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE incidents SET title = $3, severity = $4, status = $5, owner_id = $6, \
             public = $7, resolved_ms = $8 \
             WHERE org_id = $1 AND id = $2",
        )
        .bind(incident.org.as_uuid())
        .bind(incident.id.as_uuid())
        .bind(incident.title.as_str())
        .bind(incident.severity.as_str())
        .bind(incident.status.as_str())
        .bind(incident.owner.map(AccountId::as_uuid))
        .bind(incident.public)
        .bind(incident.resolved_ms.map(signed))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("change an incident"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn following(&self, org: TenantId, rule: RuleId) -> Result<Vec<IncidentId>, StoreError> {
        let mut tx = self.tx.lock().await;
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM incidents \
             WHERE org_id = $1 AND opened_from_alert = $2 AND status <> 'resolved' \
             ORDER BY opened_ms, id",
        )
        .bind(org.as_uuid())
        .bind(rule.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("find the incidents following an alert"))?;
        Ok(ids.into_iter().map(IncidentId::new).collect())
    }
}

#[repository(domain = IncidentTimeline, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresIncidentTimeline {
    tx: SharedTx,
}

impl PostgresIncidentTimeline {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

fn read_entry(
    row: &PgRow,
    org: TenantId,
    incident: IncidentId,
) -> Result<TimelineEntry, StoreError> {
    let author = author_of(
        row.get::<&str, _>("author_kind"),
        row.get("author_id"),
        row.get::<Option<&str>, _>("author_label"),
    )
    .ok_or_else(|| refused("author"))?;
    let change = decode_change(
        row.get::<&str, _>("kind"),
        row.get::<Option<&str>, _>("body"),
        row.get::<&str, _>("detail"),
        incident,
    )
    .ok_or_else(|| refused("detail"))?;
    Ok(TimelineEntry {
        id: TimelineId::new(row.get("id")),
        incident,
        org,
        author,
        change,
        at_ms: unsigned(row.get("at_ms")),
    })
}

impl IncidentTimeline for PostgresIncidentTimeline {
    async fn append(&self, entry: &TimelineEntry) -> Result<(), StoreError> {
        let (author_kind, author_id, author_label) = author_parts(&entry.author);
        let (body, detail) = encode_change(&entry.change);
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO incident_events \
             (id, incident_id, org_id, kind, author_kind, author_id, author_label, body, detail, at_ms) \
             SELECT $1, i.id, i.org_id, $4, $5, $6, $7, $8, $9::jsonb, $10 \
             FROM incidents i WHERE i.id = $2 AND i.org_id = $3",
        )
        .bind(entry.id.as_uuid())
        .bind(entry.incident.as_uuid())
        .bind(entry.org.as_uuid())
        .bind(entry.change.kind().as_str())
        .bind(author_kind)
        .bind(author_id)
        .bind(author_label)
        .bind(body)
        .bind(detail)
        .bind(signed(entry.at_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("append to an incident timeline"))?;
        Ok(())
    }

    async fn of(
        &self,
        org: TenantId,
        incident: IncidentId,
    ) -> Result<Vec<TimelineEntry>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id, kind, author_kind, author_id, author_label, body, detail::text AS detail, at_ms \
             FROM incident_events WHERE org_id = $1 AND incident_id = $2 ORDER BY seq",
        )
        .bind(org.as_uuid())
        .bind(incident.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("read an incident timeline"))?;
        rows.iter()
            .map(|row| read_entry(row, org, incident))
            .collect()
    }
}

#[repository(domain = IncidentAttachments, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresIncidentAttachments {
    tx: SharedTx,
}

impl PostgresIncidentAttachments {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

fn read_attachment(row: &PgRow, incident: IncidentId) -> Result<Attachment, StoreError> {
    let kind = AttachmentKind::parse(row.get::<&str, _>("kind")).ok_or_else(|| refused("kind"))?;
    Ok(Attachment {
        id: AttachmentId::new(row.get("id")),
        incident,
        reference: Reference::parse(kind, row.get::<&str, _>("reference"))
            .map_err(|_| refused("reference"))?,
        label: AttachmentLabel::try_from(row.get::<&str, _>("label"))
            .map_err(|_| refused("label"))?,
        at_ms: unsigned(row.get("at_ms")),
    })
}

impl IncidentAttachments for PostgresIncidentAttachments {
    async fn attach(&self, org: TenantId, attachment: &Attachment) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO incident_attachments \
             (id, incident_id, org_id, kind, reference, label, at_ms) \
             SELECT $1, i.id, i.org_id, $4, $5, $6, $7 \
             FROM incidents i WHERE i.id = $2 AND i.org_id = $3",
        )
        .bind(attachment.id.as_uuid())
        .bind(attachment.incident.as_uuid())
        .bind(org.as_uuid())
        .bind(attachment.kind().as_str())
        .bind(attachment.reference.as_str())
        .bind(attachment.label.as_str())
        .bind(signed(attachment.at_ms))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("attach to an incident"))?;
        Ok(())
    }

    async fn of(&self, org: TenantId, incident: IncidentId) -> Result<Vec<Attachment>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id, kind, reference, label, at_ms FROM incident_attachments \
             WHERE org_id = $1 AND incident_id = $2 ORDER BY at_ms, id",
        )
        .bind(org.as_uuid())
        .bind(incident.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("read the attachments of an incident"))?;
        rows.iter()
            .map(|row| read_attachment(row, incident))
            .collect()
    }
}
