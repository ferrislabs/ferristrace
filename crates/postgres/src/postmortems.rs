use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, IncidentId, Postmortem, PostmortemBody, PostmortemId, PostmortemStatus,
    PostmortemSummary, Postmortems, StoreError, TenantId,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

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
        "a stored postmortem row carries a value the domain refuses"
    );
    StoreError::Unreachable
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn read_postmortem(row: &PgRow) -> Result<Postmortem, StoreError> {
    Ok(Postmortem {
        id: PostmortemId::new(row.get("id")),
        org: TenantId::new(row.get("org_id")),
        incident: IncidentId::new(row.get("incident_id")),
        status: PostmortemStatus::parse(row.get::<&str, _>("status"))
            .ok_or_else(|| refused("status"))?,
        body: PostmortemBody::try_from(row.get::<&str, _>("body")).map_err(|_| refused("body"))?,
        author: row.get::<Option<Uuid>, _>("author_id").map(AccountId::new),
        edited: row.get("edited"),
        created_ms: unsigned(row.get("created_ms")),
        updated_ms: unsigned(row.get("updated_ms")),
        published_ms: row.get::<Option<i64>, _>("published_ms").map(unsigned),
    })
}

#[repository(domain = Postmortems, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresPostmortems {
    tx: SharedTx,
}

impl PostgresPostmortems {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl Postmortems for PostgresPostmortems {
    async fn of(
        &self,
        org: TenantId,
        incident: IncidentId,
    ) -> Result<Option<Postmortem>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "SELECT id, org_id, incident_id, status, body, author_id, edited, \
             created_ms, updated_ms, published_ms FROM postmortems \
             WHERE org_id = $1 AND incident_id = $2",
        )
        .bind(org.as_uuid())
        .bind(incident.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("read a postmortem"))?;
        row.as_ref().map(read_postmortem).transpose()
    }

    async fn summaries(&self, org: TenantId) -> Result<Vec<PostmortemSummary>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT incident_id, status FROM postmortems WHERE org_id = $1 \
             ORDER BY created_ms, id",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the postmortems"))?;
        rows.iter()
            .map(|row| {
                Ok(PostmortemSummary {
                    incident: IncidentId::new(row.get("incident_id")),
                    status: PostmortemStatus::parse(row.get::<&str, _>("status"))
                        .ok_or_else(|| refused("status"))?,
                })
            })
            .collect()
    }

    async fn create(&self, postmortem: &Postmortem) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "INSERT INTO postmortems \
             (id, org_id, incident_id, status, body, author_id, edited, \
              created_ms, updated_ms, published_ms) \
             SELECT $1, i.org_id, i.id, $4, $5, $6, $7, $8, $9, $10 \
             FROM incidents i WHERE i.id = $3 AND i.org_id = $2 \
             ON CONFLICT (incident_id) DO NOTHING",
        )
        .bind(postmortem.id.as_uuid())
        .bind(postmortem.org.as_uuid())
        .bind(postmortem.incident.as_uuid())
        .bind(postmortem.status.as_str())
        .bind(postmortem.body.as_str())
        .bind(postmortem.author.map(AccountId::as_uuid))
        .bind(postmortem.edited)
        .bind(signed(postmortem.created_ms))
        .bind(signed(postmortem.updated_ms))
        .bind(postmortem.published_ms.map(signed))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("draft a postmortem"))?;
        Ok(done.rows_affected() == 1)
    }

    async fn keep(&self, postmortem: &Postmortem) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE postmortems SET status = $3, body = $4, author_id = $5, edited = $6, \
             updated_ms = $7, published_ms = $8 \
             WHERE org_id = $1 AND id = $2",
        )
        .bind(postmortem.org.as_uuid())
        .bind(postmortem.id.as_uuid())
        .bind(postmortem.status.as_str())
        .bind(postmortem.body.as_str())
        .bind(postmortem.author.map(AccountId::as_uuid))
        .bind(postmortem.edited)
        .bind(signed(postmortem.updated_ms))
        .bind(postmortem.published_ms.map(signed))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("change a postmortem"))?;
        Ok(done.rows_affected() == 1)
    }
}
