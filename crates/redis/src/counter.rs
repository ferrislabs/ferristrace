use core::fmt;

use ferristrace_domain::{Clock, DecompressedBytes, TenantId, UsageMeter, UsageReader};
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use sqlx::{PgPool, Row};

use ferristrace_infra::{BillingHour, BillingMonth};

const KEY_PREFIX: &str = "usage";
const KEY_TTL_SECONDS: i64 = 70 * 24 * 60 * 60;
const HOUR_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
const SCAN_BATCH: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum CounterError {
    #[error("the usage counter store refused the command")]
    Redis(#[from] redis::RedisError),
    #[error("the control plane refused the usage query")]
    ControlPlane(#[from] sqlx::Error),
}

pub struct RedisMeter<C> {
    prefix: String,
    connection: ConnectionManager,
    clock: C,
}

impl<C> fmt::Debug for RedisMeter<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("RedisMeter").finish_non_exhaustive()
    }
}

impl<C: Clock> RedisMeter<C> {
    pub async fn connect(url: &str, clock: C) -> Result<Self, CounterError> {
        let client = redis::Client::open(url)?;
        let connection = ConnectionManager::new(client).await?;
        Ok(Self {
            prefix: KEY_PREFIX.to_owned(),
            connection,
            clock,
        })
    }

    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    fn key(&self, tenant: TenantId) -> String {
        let month = BillingMonth::containing(self.clock.now()).key();
        format!("{}:{tenant}:{month}", self.prefix)
    }

    fn hour_key(&self, tenant: TenantId) -> String {
        let hour = BillingHour::containing(self.clock.now()).key();
        format!("{}:{tenant}:{hour}", self.prefix)
    }

    pub async fn roll_up(&self, pool: &PgPool) -> Result<usize, CounterError> {
        let hour = BillingHour::containing(self.clock.now()).previous();
        let pattern = format!("{}:*:{}", self.prefix, hour.key());
        let mut connection = self.connection.clone();
        let mut cursor: u64 = 0;
        let mut rolled = 0;

        loop {
            let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(SCAN_BATCH)
                .query_async(&mut connection)
                .await?;

            for key in keys {
                let Some(tenant) = tenant_of(&key) else {
                    continue;
                };
                let Some(total) = connection.get::<_, Option<u64>>(&key).await? else {
                    continue;
                };
                let written = sqlx::query(
                    "INSERT INTO usage_hourly (org_id, hour_start, ingested_bytes) \
                     SELECT $1, $2, $3 WHERE EXISTS (SELECT 1 FROM orgs WHERE id = $1) \
                     ON CONFLICT (org_id, hour_start) DO NOTHING",
                )
                .bind(tenant.as_uuid())
                .bind(hour.start_nanos())
                .bind(i64::try_from(total).unwrap_or(i64::MAX))
                .execute(pool)
                .await?;
                let _: u64 = connection.del(&key).await?;
                if written.rows_affected() > 0 {
                    rolled += 1;
                }
            }

            cursor = next;
            if cursor == 0 {
                break;
            }
        }
        Ok(rolled)
    }

    pub async fn seed(
        &self,
        tenant: TenantId,
        total: DecompressedBytes,
    ) -> Result<bool, CounterError> {
        let mut connection = self.connection.clone();
        let key = self.key(tenant);
        let claimed: bool = connection.set_nx(&key, total.get()).await?;
        if claimed {
            let _: bool = connection.expire(&key, KEY_TTL_SECONDS).await?;
        }
        Ok(claimed)
    }

    pub async fn rehydrate(&self, pool: &PgPool) -> Result<usize, CounterError> {
        let since = BillingMonth::containing(self.clock.now()).start_nanos();
        let rows = sqlx::query(
            "SELECT org_id AS org_id, sum(ingested_bytes)::bigint AS total \
             FROM usage_hourly WHERE hour_start >= $1 GROUP BY org_id",
        )
        .bind(since)
        .fetch_all(pool)
        .await?;

        let mut seeded = 0;
        for row in rows {
            let tenant = TenantId::new(row.get("org_id"));
            let total = u64::try_from(row.get::<i64, _>("total")).unwrap_or_default();
            if self.seed(tenant, DecompressedBytes::new(total)).await? {
                seeded += 1;
            }
        }
        Ok(seeded)
    }
}

impl<C: Clock + Send + Sync> UsageMeter for RedisMeter<C> {
    async fn record_ingest(&self, tenant: TenantId, decompressed: DecompressedBytes) {
        let mut connection = self.connection.clone();
        let bytes = decompressed.get();
        let month = self.key(tenant);
        let hour = self.hour_key(tenant);

        let replies: Result<(u64, u64), _> = redis::pipe()
            .atomic()
            .incr(&month, bytes)
            .incr(&hour, bytes)
            .query_async(&mut connection)
            .await;

        match replies {
            Ok((month_total, hour_total)) => {
                expire_if_new(&mut connection, &month, month_total, bytes, KEY_TTL_SECONDS).await;
                expire_if_new(&mut connection, &hour, hour_total, bytes, HOUR_TTL_SECONDS).await;
            }
            Err(error) => {
                tracing::error!(%error, %tenant, "the usage counters were not incremented");
            }
        }
    }
}

async fn expire_if_new(
    connection: &mut ConnectionManager,
    key: &str,
    total: u64,
    increment: u64,
    ttl: i64,
) {
    if total != increment {
        return;
    }
    if let Err(error) = connection.expire::<_, bool>(key, ttl).await {
        tracing::error!(%error, key, "the usage counter was left without an expiry");
    }
}

fn tenant_of(key: &str) -> Option<TenantId> {
    let mut segments = key.rsplit(':');
    segments.next()?;
    segments
        .next()?
        .parse::<uuid::Uuid>()
        .ok()
        .map(TenantId::new)
}

impl<C: Clock + Send + Sync> UsageReader for RedisMeter<C> {
    async fn used(&self, tenant: TenantId) -> DecompressedBytes {
        let mut connection = self.connection.clone();
        match connection.get::<_, Option<u64>>(self.key(tenant)).await {
            Ok(total) => DecompressedBytes::new(total.unwrap_or_default()),
            Err(error) => {
                tracing::error!(
                    %error,
                    %tenant,
                    "the usage counter could not be read, so the payload is admitted"
                );
                DecompressedBytes::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tenant_of;

    #[test]
    fn a_key_names_its_tenant_whatever_the_prefix() {
        let plain = "usage:2f1e8a4c-0000-4000-8000-000000000001:202311";
        let nested = "test:rollup:2f1e8a4c-0000-4000-8000-000000000001:2023111422";

        assert_eq!(tenant_of(plain), tenant_of(nested));
        assert!(tenant_of(plain).is_some());
    }

    #[test]
    fn a_key_without_a_tenant_names_nobody() {
        assert_eq!(tenant_of("usage:202311"), None);
        assert_eq!(tenant_of("usage"), None);
    }
}
