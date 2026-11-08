use ferristrace_domain::RetentionDays;
use ferristrace_domain::{
    Clock, IngestKey, KEY_PREFIX, KeyScope, ProjectId, Scope, SecretHash, TenantDirectory,
    TenantId, UnixSeconds,
};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

const NANOS_PER_SECOND: i64 = 1_000_000_000;
pub(crate) const SIGHTING_GAP_SECONDS: u64 = 300;

#[derive(Debug, Clone)]
pub struct PostgresDirectory<C> {
    pool: PgPool,
    clock: C,
    default_retention_days: Option<u32>,
}

impl<C: Clock> PostgresDirectory<C> {
    pub fn new(pool: PgPool, clock: C) -> Self {
        Self {
            pool,
            clock,
            default_retention_days: None,
        }
    }

    #[must_use]
    pub fn with_default_retention_days(mut self, days: u64) -> Self {
        self.default_retention_days = Some(u32::try_from(days).unwrap_or(u32::MAX));
        self
    }

    async fn note_sighting(&self, key: Uuid, last_seen: Option<i64>) {
        let now = self.clock.now();
        if !sighting_is_stale(last_seen, now) {
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

pub(crate) struct Minted {
    pub(crate) id: Uuid,
    pub(crate) prefix: String,
    pub(crate) plaintext: String,
    pub(crate) created_at: i64,
    pub(crate) expires_at: Option<i64>,
}

pub(crate) struct Minting<'a> {
    pub(crate) org: TenantId,
    pub(crate) project: Option<Uuid>,
    pub(crate) name: &'a str,
    pub(crate) scopes: &'a [&'a str],
    pub(crate) by: Option<Uuid>,
    /// How many nanoseconds after this same insert's `created_at` the key
    /// expires - `None` for a key that never does.
    pub(crate) ttl_nanos: Option<i64>,
}

pub(crate) async fn mint_on(
    connection: &mut PgConnection,
    minting: Minting<'_>,
) -> Result<Minted, sqlx::Error> {
    let prefix_bytes: [u8; PREFIX_BYTES] = rand::random();
    let secret_bytes: [u8; SECRET_BYTES] = rand::random();
    let prefix = format!("{KEY_PREFIX}{}", hex::encode(prefix_bytes));
    let secret = hex::encode(secret_bytes);

    let row = sqlx::query(
        "WITH stamp AS (SELECT clock_timestamp() AS now) \
         INSERT INTO api_keys \
         (id, org_id, project_id, prefix, hash, secret_digest, name, scopes, created_by, \
          created_at, expires_at) \
         SELECT gen_random_uuid(), $1, $2, $3, '', $4, $5, $6, $7, \
                (extract(epoch FROM stamp.now) * 1000000000)::bigint, \
                CASE WHEN $8::bigint IS NULL THEN NULL \
                     ELSE (extract(epoch FROM stamp.now) * 1000000000)::bigint + $8::bigint \
                END \
         FROM stamp \
         RETURNING id AS id, created_at AS created_at, expires_at AS expires_at",
    )
    .bind(minting.org.as_uuid())
    .bind(minting.project)
    .bind(&prefix)
    .bind(SecretHash::of(&secret).as_bytes().as_slice())
    .bind(minting.name)
    .bind(minting.scopes)
    .bind(minting.by)
    .bind(minting.ttl_nanos)
    .fetch_one(connection)
    .await?;

    let plaintext = format!("{prefix}.{secret}");
    Ok(Minted {
        id: row.get("id"),
        prefix,
        plaintext,
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
    })
}

pub(crate) async fn issue_on(
    connection: &mut PgConnection,
    org: TenantId,
    project: Option<Uuid>,
) -> Result<IngestKey, KeyError> {
    let minted = ingest_on(connection, org, project).await?;
    Ok(IngestKey::from_plaintext(&minted.plaintext))
}

pub(crate) async fn ingest_on(
    connection: &mut PgConnection,
    org: TenantId,
    project: Option<Uuid>,
) -> Result<Minted, KeyError> {
    Ok(mint_on(
        connection,
        Minting {
            org,
            project,
            name: "",
            scopes: &[KeyScope::Ingest.as_str()],
            by: None,
            ttl_nanos: None,
        },
    )
    .await?)
}

pub(crate) struct Stopped {
    pub(crate) org: Uuid,
    pub(crate) id: Uuid,
}

pub(crate) async fn stop_on(
    connection: &mut PgConnection,
    prefix: &str,
    now: UnixSeconds,
) -> Result<Option<Stopped>, KeyError> {
    let stopped = sqlx::query(
        "UPDATE api_keys SET revoked_at = $2 \
         WHERE prefix = $1 AND revoked_at IS NULL \
         RETURNING org_id AS org_id, id AS id",
    )
    .bind(prefix)
    .bind(to_nanos(now))
    .fetch_optional(connection)
    .await?;
    Ok(stopped.map(|row| Stopped {
        org: row.get("org_id"),
        id: row.get("id"),
    }))
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
            "SELECT k.id AS id, k.org_id AS org_id, k.project_id AS project_id, \
             k.secret_digest AS secret_digest, k.last_used_at AS last_used_at, \
             o.retention_days AS org_retention_days, p.retention_days AS plan_retention_days \
             FROM api_keys k \
             JOIN orgs o ON o.id = k.org_id \
             JOIN plans p ON p.id = o.plan_id \
             WHERE k.prefix = $1 AND k.revoked_at IS NULL AND $2 = ANY(k.scopes)",
        )
        .bind(prefix)
        .bind(KeyScope::Ingest.as_str())
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
        let retention = effective_retention(
            row.get("org_retention_days"),
            row.get("plan_retention_days"),
            self.default_retention_days,
        );
        Ok(Scope::of(TenantId::new(row.get("org_id")))
            .in_project(row.get::<Option<Uuid>, _>("project_id").map(ProjectId::new))
            .retained_for(retention))
    }
}

fn effective_retention(
    chosen: Option<i32>,
    cap: Option<i32>,
    default: Option<u32>,
) -> Option<RetentionDays> {
    let days = |value: i32| u32::try_from(value).unwrap_or_default();
    match (chosen.map(days), cap.map(days)) {
        (Some(chosen), Some(cap)) => Some(RetentionDays::saturating(chosen.min(cap))),
        (Some(chosen), None) => Some(RetentionDays::saturating(chosen)),
        (None, Some(cap)) if default.is_none_or(|default| cap < default) => {
            Some(RetentionDays::saturating(cap))
        }
        (None, _) => None,
    }
}

pub(crate) fn digest_matches(stored: Option<Vec<u8>>, offered: &str) -> bool {
    stored
        .and_then(|stored| <[u8; 32]>::try_from(stored.as_slice()).ok())
        .is_some_and(|stored| SecretHash::from_bytes(stored) == SecretHash::of(offered))
}

pub(crate) fn sighting_is_stale(last_seen: Option<i64>, now: UnixSeconds) -> bool {
    last_seen.is_none_or(|seen| {
        nanos_to_seconds(seen)
            .saturating_add_secs(SIGHTING_GAP_SECONDS)
            .get()
            <= now.get()
    })
}

pub(crate) fn to_nanos(instant: UnixSeconds) -> i64 {
    i64::try_from(instant.get())
        .unwrap_or(i64::MAX / NANOS_PER_SECOND)
        .saturating_mul(NANOS_PER_SECOND)
}

fn nanos_to_seconds(nanos: i64) -> UnixSeconds {
    UnixSeconds::new(u64::try_from(nanos / NANOS_PER_SECOND).unwrap_or_default())
}
