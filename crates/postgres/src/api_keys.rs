use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, AgentId, ApiKey, ApiKeys, ApiSecret, AuditEntry, AuditTrail, Clock, ExpiringKey,
    ExpiryThreshold, IssuedKey, KeyId, KeyName, KeyScope, KeyScopes, KeyTtl, ProjectId, ReadGrant,
    RegisteredAgent, StoreError, TenantId,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::agents::{standing_on, touch_on};
use crate::audit::PostgresAuditTrail;
use crate::keys::{Minting, digest_matches, mint_on, sighting_is_stale, to_nanos};
use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = ApiKeys, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresApiKeys<C> {
    tx: SharedTx,
    clock: C,
    trail: PostgresAuditTrail<C>,
}

impl<C: Clock + Clone> PostgresApiKeys<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            clock: backend.clock().clone(),
            trail: PostgresAuditTrail::new(backend, tx),
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

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

const SEVEN_DAYS_NANOS: i64 = 7 * 86_400 * 1_000_000_000;
const ONE_DAY_NANOS: i64 = 86_400 * 1_000_000_000;

fn read_expiring(row: &PgRow) -> ExpiringKey {
    ExpiringKey {
        id: KeyId::new(row.get("id")),
        org: TenantId::new(row.get("org_id")),
        name: row.get::<String, _>("name").into(),
        created_by: row.get::<Option<Uuid>, _>("created_by").map(AccountId::new),
    }
}

/// The requested duration in nanoseconds - `KeyTtl::expires_at` with a zero
/// base, since all we want here is the span, not an absolute instant.
fn ttl_nanos(ttl: KeyTtl) -> Option<i64> {
    ttl.expires_at(0)
        .map(|duration| i64::try_from(duration).unwrap_or(i64::MAX))
}

fn read_key(row: &PgRow) -> Result<ApiKey, StoreError> {
    let spelled: Vec<String> = row.get("scopes");
    let scopes = KeyScopes::parse(&spelled).map_err(|error| {
        tracing::error!(%error, "a stored key carries scopes the domain refuses");
        StoreError::Unreachable
    })?;
    Ok(ApiKey {
        id: KeyId::new(row.get("id")),
        project: row.get::<Option<Uuid>, _>("project_id").map(ProjectId::new),
        name: row.get::<String, _>("name").into(),
        prefix: row.get::<String, _>("prefix").into(),
        scopes,
        created_at_unix_nanos: nanos(row.get("created_at")),
        created_by: row.get::<Option<Uuid>, _>("created_by").map(AccountId::new),
        last_used_at_unix_nanos: row.get::<Option<i64>, _>("last_used_at").map(nanos),
        expires_at_unix_nanos: row.get::<Option<i64>, _>("expires_at").map(nanos),
    })
}

impl<C: Clock + Clone + Send + Sync> ApiKeys for PostgresApiKeys<C> {
    async fn list(&self, org: TenantId) -> Result<Vec<ApiKey>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id AS id, project_id AS project_id, name AS name, prefix AS prefix, \
             scopes AS scopes, created_at AS created_at, created_by AS created_by, \
             last_used_at AS last_used_at, expires_at AS expires_at \
             FROM api_keys WHERE org_id = $1 AND revoked_at IS NULL \
             ORDER BY created_at DESC, prefix",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the keys"))?;
        rows.iter().map(read_key).collect()
    }

    async fn issue(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        by: AccountId,
        ttl: KeyTtl,
    ) -> Result<IssuedKey, StoreError> {
        let mut tx = self.tx.lock().await;
        let spelled: Vec<&str> = scopes
            .as_slice()
            .iter()
            .map(|scope| scope.as_str())
            .collect();
        let minted = mint_on(
            &mut tx,
            Minting {
                org,
                project: project.map(ProjectId::as_uuid),
                name: name.as_str(),
                scopes: &spelled,
                by: Some(by.as_uuid()),
                ttl_nanos: ttl_nanos(ttl),
            },
        )
        .await
        .map_err(unreachable("issue a key"))?;
        Ok(IssuedKey {
            key: ApiKey {
                id: KeyId::new(minted.id),
                project,
                name: name.as_str().into(),
                prefix: minted.prefix.into(),
                scopes: scopes.clone(),
                created_at_unix_nanos: nanos(minted.created_at),
                created_by: Some(by),
                last_used_at_unix_nanos: None,
                expires_at_unix_nanos: minted.expires_at.map(nanos),
            },
            secret: ApiSecret::from_plaintext(&minted.plaintext),
        })
    }

    async fn revoke(&self, org: TenantId, id: KeyId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query(
            "UPDATE api_keys SET revoked_at = $3 \
             WHERE org_id = $1 AND id = $2 AND revoked_at IS NULL",
        )
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .bind(to_nanos(self.clock.now()))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("revoke a key"))?;
        Ok(done.rows_affected() > 0)
    }

    async fn grant_for(&self, secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        let Some((prefix, offered)) = secret.split() else {
            return Ok(None);
        };
        let mut tx = self.tx.lock().await;
        let found = sqlx::query(
            "SELECT id AS id, org_id AS org_id, project_id AS project_id, \
             secret_digest AS secret_digest, last_used_at AS last_used_at \
             FROM api_keys \
             WHERE prefix = $1 AND revoked_at IS NULL AND $2 = ANY(scopes)",
        )
        .bind(prefix)
        .bind(KeyScope::Read.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("look a key up"))?;
        let Some(row) = found else {
            return Ok(None);
        };
        if !digest_matches(row.get("secret_digest"), offered) {
            return Ok(None);
        }
        let now = self.clock.now();
        if sighting_is_stale(row.get("last_used_at"), now) {
            sqlx::query("UPDATE api_keys SET last_used_at = $2 WHERE id = $1")
                .bind(row.get::<Uuid, _>("id"))
                .bind(to_nanos(now))
                .execute(&mut **tx)
                .await
                .map_err(unreachable("note a key's use"))?;
        }
        Ok(Some(ReadGrant {
            org: TenantId::new(row.get("org_id")),
            project: row.get::<Option<Uuid>, _>("project_id").map(ProjectId::new),
        }))
    }

    async fn due_for_expiry_notice(
        &self,
        now_unix_nanos: u64,
    ) -> Result<Vec<(ExpiringKey, ExpiryThreshold)>, StoreError> {
        let mut tx = self.tx.lock().await;
        let now = signed(now_unix_nanos);
        let seven = sqlx::query(
            "SELECT id AS id, org_id AS org_id, name AS name, created_by AS created_by \
             FROM api_keys WHERE revoked_at IS NULL AND expires_at IS NOT NULL \
             AND NOT notified_7d AND expires_at > $1 AND expires_at - $1 <= $2",
        )
        .bind(now)
        .bind(SEVEN_DAYS_NANOS)
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("find keys entering their seven day notice"))?;
        let one = sqlx::query(
            "SELECT id AS id, org_id AS org_id, name AS name, created_by AS created_by \
             FROM api_keys WHERE revoked_at IS NULL AND expires_at IS NOT NULL \
             AND NOT notified_1d AND expires_at > $1 AND expires_at - $1 <= $2",
        )
        .bind(now)
        .bind(ONE_DAY_NANOS)
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("find keys entering their one day notice"))?;
        Ok(seven
            .iter()
            .map(|row| (read_expiring(row), ExpiryThreshold::Days7))
            .chain(
                one.iter()
                    .map(|row| (read_expiring(row), ExpiryThreshold::Day1)),
            )
            .collect())
    }

    async fn mark_expiry_notice_sent(
        &self,
        id: KeyId,
        threshold: ExpiryThreshold,
    ) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        let query = match threshold {
            ExpiryThreshold::Days7 => "UPDATE api_keys SET notified_7d = true WHERE id = $1",
            ExpiryThreshold::Day1 => "UPDATE api_keys SET notified_1d = true WHERE id = $1",
        };
        sqlx::query(query)
            .bind(id.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(unreachable("mark a key's expiry notice sent"))?;
        Ok(())
    }

    async fn due_for_auto_revoke(
        &self,
        now_unix_nanos: u64,
    ) -> Result<Vec<ExpiringKey>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id AS id, org_id AS org_id, name AS name, created_by AS created_by \
             FROM api_keys WHERE revoked_at IS NULL AND expires_at IS NOT NULL \
             AND expires_at <= $1",
        )
        .bind(signed(now_unix_nanos))
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("find keys past their deadline"))?;
        Ok(rows.iter().map(read_expiring).collect())
    }

    async fn agent_standing(
        &self,
        org: TenantId,
        client_id: &str,
    ) -> Result<Option<RegisteredAgent>, StoreError> {
        standing_on(&self.tx, org, client_id).await
    }

    async fn agent_used(
        &self,
        org: TenantId,
        agent: AgentId,
        entry: &AuditEntry,
    ) -> Result<(), StoreError> {
        touch_on(&self.tx, &self.clock, org, agent).await?;
        self.trail.record(entry).await
    }
}
