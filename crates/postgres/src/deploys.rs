use ferristrace_application::repository;
use ferristrace_domain::{DeployMarks, StoreError};

use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = DeployMarks, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresDeployMarks {
    tx: SharedTx,
}

impl PostgresDeployMarks {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

fn unreachable(doing: &'static str) -> impl FnOnce(sqlx::Error) -> StoreError {
    move |error| {
        tracing::error!(%error, "the control plane refused to {doing}");
        StoreError::Unreachable
    }
}

impl DeployMarks for PostgresDeployMarks {
    async fn watermark(&self) -> Result<Option<u64>, StoreError> {
        let mut tx = self.tx.lock().await;
        let held: Option<i64> = sqlx::query_scalar("SELECT at_ms FROM deploy_watermark")
            .fetch_optional(&mut **tx)
            .await
            .map_err(unreachable("read the deploy watermark"))?;
        Ok(held.map(|at_ms| u64::try_from(at_ms).unwrap_or_default()))
    }

    async fn advance(&self, to_ms: u64) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "INSERT INTO deploy_watermark (singleton, at_ms) VALUES (true, $1) \
             ON CONFLICT (singleton) DO UPDATE \
             SET at_ms = GREATEST(deploy_watermark.at_ms, EXCLUDED.at_ms)",
        )
        .bind(i64::try_from(to_ms).unwrap_or(i64::MAX))
        .execute(&mut **tx)
        .await
        .map_err(unreachable("advance the deploy watermark"))?;
        Ok(())
    }
}
