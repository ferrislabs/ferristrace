use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, AgentEnrolment, AgentId, AgentScopes, Agents, Clock, ProjectId, RegisteredAgent,
    StoreError, TenantId,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::keys::{SIGHTING_GAP_SECONDS, to_nanos};
use crate::unit_of_work::{Postgres, SharedTx};

macro_rules! columns {
    () => {
        "id AS id, org_id AS org_id, name AS name, client_id AS client_id, \
         scopes AS scopes, project_id AS project_id, created_by AS created_by, \
         created_at AS created_at, last_used_at AS last_used_at, revoked_at AS revoked_at"
    };
}

const NANOS_PER_SECOND: i64 = 1_000_000_000;

#[repository(domain = Agents, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresAgents<C> {
    tx: SharedTx,
    clock: C,
}

impl<C: Clock + Clone> PostgresAgents<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            clock: backend.clock().clone(),
        }
    }
}

fn unreachable(doing: &'static str) -> impl FnOnce(sqlx::Error) -> StoreError {
    move |error| {
        tracing::error!(%error, "the control plane refused to {doing}");
        StoreError::Unreachable
    }
}

fn nanos(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn read_agent(row: &PgRow) -> Result<RegisteredAgent, StoreError> {
    let spelled: Vec<String> = row.get("scopes");
    let scopes = AgentScopes::parse(&spelled).map_err(|error| {
        tracing::error!(%error, "a stored agent carries scopes the domain refuses");
        StoreError::Unreachable
    })?;
    Ok(RegisteredAgent {
        id: AgentId::new(row.get("id")),
        org: TenantId::new(row.get("org_id")),
        name: row.get::<String, _>("name").into(),
        client_id: row.get::<String, _>("client_id").into(),
        scopes,
        project: row.get::<Option<Uuid>, _>("project_id").map(ProjectId::new),
        created_by: row.get::<Option<Uuid>, _>("created_by").map(AccountId::new),
        created_at_unix_nanos: nanos(row.get("created_at")),
        last_used_at_unix_nanos: row.get::<Option<i64>, _>("last_used_at").map(nanos),
        revoked_at_unix_nanos: row.get::<Option<i64>, _>("revoked_at").map(nanos),
    })
}

pub(crate) async fn standing_on(
    tx: &SharedTx,
    org: TenantId,
    client_id: &str,
) -> Result<Option<RegisteredAgent>, StoreError> {
    let mut tx = tx.lock().await;
    let found = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM agents WHERE org_id = $1 AND client_id = $2 \
         ORDER BY (revoked_at IS NULL) DESC, created_at DESC LIMIT 1"
    ))
    .bind(org.as_uuid())
    .bind(client_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(unreachable("look an agent up"))?;
    found.as_ref().map(read_agent).transpose()
}

pub(crate) async fn touch_on<C: Clock>(
    tx: &SharedTx,
    clock: &C,
    org: TenantId,
    id: AgentId,
) -> Result<(), StoreError> {
    let now = to_nanos(clock.now());
    let gap = i64::try_from(SIGHTING_GAP_SECONDS)
        .unwrap_or(i64::MAX / NANOS_PER_SECOND)
        .saturating_mul(NANOS_PER_SECOND);
    let mut tx = tx.lock().await;
    sqlx::query(
        "UPDATE agents SET last_used_at = $3 \
         WHERE org_id = $1 AND id = $2 AND (last_used_at IS NULL OR last_used_at <= $3 - $4)",
    )
    .bind(org.as_uuid())
    .bind(id.as_uuid())
    .bind(now)
    .bind(gap)
    .execute(&mut **tx)
    .await
    .map_err(unreachable("note an agent's use"))?;
    Ok(())
}

impl<C: Clock + Clone + Send + Sync> Agents for PostgresAgents<C> {
    async fn list(&self, org: TenantId) -> Result<Vec<RegisteredAgent>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM agents WHERE org_id = $1 \
             ORDER BY (revoked_at IS NULL) DESC, created_at DESC, id"
        ))
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the agents"))?;
        rows.iter().map(read_agent).collect()
    }

    async fn register(
        &self,
        org: TenantId,
        enrolment: &AgentEnrolment,
        by: AccountId,
    ) -> Result<Option<RegisteredAgent>, StoreError> {
        let spelled: Vec<&str> = enrolment
            .scopes
            .as_slice()
            .iter()
            .map(|scope| scope.as_str())
            .collect();
        let mut tx = self.tx.lock().await;
        let inserted = sqlx::query(concat!(
            "INSERT INTO agents \
             (id, org_id, name, client_id, scopes, project_id, created_by, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (org_id, client_id) WHERE revoked_at IS NULL DO NOTHING \
             RETURNING ",
            columns!()
        ))
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(enrolment.name.as_str())
        .bind(enrolment.client_id.as_str())
        .bind(spelled)
        .bind(enrolment.project.map(ProjectId::as_uuid))
        .bind(by.as_uuid())
        .bind(to_nanos(self.clock.now()))
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("register an agent"))?;
        inserted.as_ref().map(read_agent).transpose()
    }

    async fn revoke(
        &self,
        org: TenantId,
        id: AgentId,
    ) -> Result<Option<RegisteredAgent>, StoreError> {
        let mut tx = self.tx.lock().await;
        let revoked = sqlx::query(concat!(
            "UPDATE agents SET revoked_at = $3 \
             WHERE org_id = $1 AND id = $2 AND revoked_at IS NULL \
             RETURNING ",
            columns!()
        ))
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .bind(to_nanos(self.clock.now()))
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("revoke an agent"))?;
        revoked.as_ref().map(read_agent).transpose()
    }
}
