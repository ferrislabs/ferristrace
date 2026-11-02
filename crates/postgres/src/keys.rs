use ferristrace_domain::{
    Clock, IngestKey, ProjectId, Scope, SecretHash, TenantDirectory, TenantId, UnixSeconds,
};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const SIGHTING_GAP_SECONDS: u64 = 300;

#[derive(Debug, Clone)]
pub struct PostgresDirectory<C> {
    pool: PgPool,
    clock: C,
}

impl<C: Clock> PostgresDirectory<C> {
    pub fn new(pool: PgPool, clock: C) -> Self {
        Self { pool, clock }
    }

    async fn note_sighting(&self, key: Uuid, last_seen: Option<i64>) {
        let now = self.clock.now();
        let stale = last_seen.is_none_or(|seen| {
            nanos_to_seconds(seen)
                .saturating_add_secs(SIGHTING_GAP_SECONDS)
                .get()
                <= now.get()
        });
        if !stale {
            return;
        }
        let done = sqlx::query("UPDATE api_keys SET last_used_at = $2 WHERE id = $1")
            .bind(key)
            .bind(to_nanos(now))
            .execute(&self.pool)
            .await;
        if let Err(error) = done {
            tracing::warn!(%error, "a key sighting was not recorded");
        }
    }
}

const PREFIX_LABEL: &str = "ct_live_";
const PREFIX_BYTES: usize = 6;
const SECRET_BYTES: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("the control plane refused the key")]
    ControlPlane(#[from] sqlx::Error),
}

impl<C: Clock> PostgresDirectory<C> {
    pub async fn issue(&self, org: TenantId, project: Option<Uuid>) -> Result<IngestKey, KeyError> {
        let mut connection = self.pool.acquire().await?;
        issue_on(&mut connection, org, project).await
    }

    pub async fn live_keys(&self, org: TenantId) -> Result<i64, KeyError> {
        let counted = sqlx::query(
            "SELECT count(*) AS n FROM api_keys \
             WHERE org_id = $1 AND revoked_at IS NULL AND secret_digest IS NOT NULL",
        )
        .bind(org.as_uuid())
        .fetch_one(&self.pool)
        .await?;
        Ok(counted.get("n"))
    }

    pub async fn revoke(&self, prefix: &str) -> Result<bool, KeyError> {
        let mut connection = self.pool.acquire().await?;
        revoke_on(&mut connection, prefix, self.clock.now()).await
    }
}

pub(crate) async fn issue_on(
    connection: &mut PgConnection,
    org: TenantId,
    project: Option<Uuid>,
) -> Result<IngestKey, KeyError> {
    let prefix_bytes: [u8; PREFIX_BYTES] = rand::random();
    let secret_bytes: [u8; SECRET_BYTES] = rand::random();
    let prefix = format!("{PREFIX_LABEL}{}", hex::encode(prefix_bytes));
    let secret = hex::encode(secret_bytes);

    sqlx::query(
        "INSERT INTO api_keys (id, org_id, project_id, prefix, hash, secret_digest) \
         VALUES (gen_random_uuid(), $1, $2, $3, '', $4)",
    )
    .bind(org.as_uuid())
    .bind(project)
    .bind(&prefix)
    .bind(SecretHash::of(&secret).as_bytes().as_slice())
    .execute(connection)
    .await?;

    Ok(IngestKey::from_plaintext(&format!("{prefix}.{secret}")))
}

pub(crate) async fn revoke_on(
    connection: &mut PgConnection,
    prefix: &str,
    now: UnixSeconds,
) -> Result<bool, KeyError> {
    let done = sqlx::query(
        "UPDATE api_keys SET revoked_at = $2 \
         WHERE prefix = $1 AND revoked_at IS NULL",
    )
    .bind(prefix)
    .bind(to_nanos(now))
    .execute(connection)
    .await?;
    Ok(done.rows_affected() > 0)
}

impl<C: Clock + Send + Sync> TenantDirectory for PostgresDirectory<C> {
    async fn resolve(&self, key: &IngestKey) -> Result<Scope, ferristrace_domain::UnknownKey> {
        let Some((prefix, secret)) = key.split() else {
            return Err(ferristrace_domain::UnknownKey);
        };

        let found = sqlx::query(
            "SELECT id AS id, org_id AS org_id, project_id AS project_id, \
             secret_digest AS secret_digest, last_used_at AS last_used_at \
             FROM api_keys WHERE prefix = $1 AND revoked_at IS NULL",
        )
        .bind(prefix)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused a key lookup");
            ferristrace_domain::UnknownKey
        })?;

        let offered = SecretHash::of(secret);
        let Some(row) = found else {
            return Err(ferristrace_domain::UnknownKey);
        };
        let Some(stored) = row.get::<Option<Vec<u8>>, _>("secret_digest") else {
            return Err(ferristrace_domain::UnknownKey);
        };
        let Ok(stored) = <[u8; 32]>::try_from(stored.as_slice()) else {
            return Err(ferristrace_domain::UnknownKey);
        };
        if SecretHash::from_bytes(stored) != offered {
            return Err(ferristrace_domain::UnknownKey);
        }

        self.note_sighting(row.get("id"), row.get("last_used_at"))
            .await;
        Ok(Scope::of(TenantId::new(row.get("org_id")))
            .in_project(row.get::<Option<Uuid>, _>("project_id").map(ProjectId::new)))
    }
}

fn to_nanos(instant: UnixSeconds) -> i64 {
    i64::try_from(instant.get())
        .unwrap_or(i64::MAX / NANOS_PER_SECOND)
        .saturating_mul(NANOS_PER_SECOND)
}

fn nanos_to_seconds(nanos: i64) -> UnixSeconds {
    UnixSeconds::new(u64::try_from(nanos / NANOS_PER_SECOND).unwrap_or_default())
}
