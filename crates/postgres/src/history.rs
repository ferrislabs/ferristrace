use ferristrace_application::repository;
use ferristrace_domain::telemetry::{BillingMonth, HourTotal, MonthTotal, Split, UsageLedger};
use ferristrace_domain::{Consumed, DecompressedBytes, StoreError, TenantId, UsageHistory, Window};
use sqlx::Row;
use uuid::Uuid;

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
            "SELECT hour_start AS hour_start, ingested_bytes AS ingested_bytes, \
             spans_bytes AS spans_bytes, logs_bytes AS logs_bytes, \
             metrics_points AS metrics_points \
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
                split: split_of(&row),
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

impl UsageLedger for PostgresUsage {
    async fn record_hours(&self, hours: &[HourTotal]) -> Result<usize, StoreError> {
        let rows: Vec<(Uuid, i64, i64, Option<Split>)> = hours
            .iter()
            .map(|total| {
                (
                    total.tenant.as_uuid(),
                    total.hour.start_nanos(),
                    i64::try_from(total.ingested.get()).unwrap_or(i64::MAX),
                    total.split,
                )
            })
            .collect();

        let mut tx = self.tx.lock().await;
        let mut written = 0;
        for (org, hour_start, ingested, split) in rows {
            let inserted = sqlx::query(
                "INSERT INTO usage_hourly \
                 (org_id, hour_start, ingested_bytes, spans_bytes, logs_bytes, metrics_points) \
                 SELECT $1, $2, $3, $4, $5, $6 WHERE EXISTS (SELECT 1 FROM orgs WHERE id = $1) \
                 ON CONFLICT (org_id, hour_start) DO NOTHING",
            )
            .bind(org)
            .bind(hour_start)
            .bind(ingested)
            .bind(split.map(|found| as_nanos(found.spans_bytes)))
            .bind(split.map(|found| as_nanos(found.logs_bytes)))
            .bind(split.map(|found| as_nanos(found.metrics_points)))
            .execute(&mut **tx)
            .await
            .map_err(|error| unreachable(&error, "roll an hour up"))?;
            if inserted.rows_affected() > 0 {
                written += 1;
            }
        }
        Ok(written)
    }

    async fn billed_since(&self, month: BillingMonth) -> Result<Vec<MonthTotal>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT org_id AS org_id, sum(ingested_bytes)::bigint AS total \
             FROM usage_hourly WHERE hour_start >= $1 GROUP BY org_id",
        )
        .bind(month.start_nanos())
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "read the billed hours"))?;

        Ok(rows
            .into_iter()
            .map(|row| MonthTotal {
                tenant: TenantId::new(row.get("org_id")),
                ingested: DecompressedBytes::new(
                    u64::try_from(row.get::<i64, _>("total")).unwrap_or_default(),
                ),
            })
            .collect())
    }
}

fn split_of(row: &sqlx::postgres::PgRow) -> Option<Split> {
    let count = |column: &str| {
        row.get::<Option<i64>, _>(column)
            .map(|value| u64::try_from(value).unwrap_or_default())
    };
    Some(Split {
        spans_bytes: count("spans_bytes")?,
        logs_bytes: count("logs_bytes")?,
        metrics_points: count("metrics_points")?,
    })
}

fn as_nanos(unsigned: u64) -> i64 {
    i64::try_from(unsigned).unwrap_or(i64::MAX)
}

fn unreachable(error: &sqlx::Error, what: &str) -> StoreError {
    tracing::error!(%error, "the control plane refused to {what}");
    StoreError::Unreachable
}
