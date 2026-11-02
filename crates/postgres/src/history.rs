use ferristrace_application::repository;
use ferristrace_domain::{Consumed, DecompressedBytes, StoreError, TenantId, UsageHistory, Window};
use sqlx::Row;

use crate::postgres::gigabytes_to_bytes;
use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = UsageHistory, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresUsage {
    tx: SharedTx,
}

impl PostgresUsage {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl UsageHistory for PostgresUsage {
    async fn hours(&self, org: TenantId, window: Window) -> Result<Vec<Consumed>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT hour_start AS hour_start, ingested_bytes AS ingested_bytes \
             FROM usage_hourly WHERE org_id = $1 AND hour_start >= $2 AND hour_start < $3 \
             ORDER BY hour_start",
        )
        .bind(org.as_uuid())
        .bind(as_nanos(window.from_unix_nanos()))
        .bind(as_nanos(window.to_unix_nanos()))
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "read the usage history"))?;

        Ok(rows
            .into_iter()
            .map(|row| Consumed {
                started_at_unix_nanos: u64::try_from(row.get::<i64, _>("hour_start"))
                    .unwrap_or_default(),
                ingested_bytes: u64::try_from(row.get::<i64, _>("ingested_bytes"))
                    .unwrap_or_default(),
            })
            .collect())
    }

    async fn included(&self, org: TenantId) -> Result<Option<DecompressedBytes>, StoreError> {
        let mut tx = self.tx.lock().await;
        let found = sqlx::query(
            "SELECT p.ingest_gb AS ingest_gb FROM orgs o JOIN plans p ON p.id = o.plan_id \
             WHERE o.id = $1",
        )
        .bind(org.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "read what a plan allows"))?;

        Ok(found
            .and_then(|row| row.get::<Option<i32>, _>("ingest_gb"))
            .and_then(gigabytes_to_bytes))
    }
}

fn as_nanos(unsigned: u64) -> i64 {
    i64::try_from(unsigned).unwrap_or(i64::MAX)
}

fn unreachable(error: &sqlx::Error, what: &str) -> StoreError {
    tracing::error!(%error, "the control plane refused to {what}");
    StoreError::Unreachable
}
