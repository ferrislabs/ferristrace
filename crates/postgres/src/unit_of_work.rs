use core::future::Future;
use std::sync::Arc;

use ferristrace_application::registry::UnitOfWork;
use sqlx::{PgPool, Transaction};
use tokio::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone)]
pub struct SharedTx(Arc<Mutex<Transaction<'static, sqlx::Postgres>>>);

impl SharedTx {
    pub async fn lock(&self) -> MutexGuard<'_, Transaction<'static, sqlx::Postgres>> {
        self.0.lock().await
    }

    fn into_transaction(self) -> Option<Transaction<'static, sqlx::Postgres>> {
        Arc::try_unwrap(self.0).ok().map(Mutex::into_inner)
    }
}

#[derive(Debug, Clone)]
pub struct Postgres<C> {
    pool: PgPool,
    clock: C,
}

impl<C> Postgres<C> {
    pub fn new(pool: PgPool, clock: C) -> Self {
        Self { pool, clock }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn clock(&self) -> &C {
        &self.clock
    }

    pub async fn scratch<T, F, Fut>(&self, work: F) -> Result<T, sqlx::Error>
    where
        F: FnOnce(SharedTx) -> Fut,
        Fut: Future<Output = T>,
    {
        let shared = SharedTx(Arc::new(Mutex::new(self.pool.begin().await?)));
        let outcome = work(shared.clone()).await;
        if let Some(tx) = shared.into_transaction() {
            tx.rollback().await?;
        }
        Ok(outcome)
    }
}

impl<C: Send + Sync + 'static> UnitOfWork for Postgres<C> {
    type Tx = SharedTx;

    fn transaction<T, E, F, Fut>(
        &self,
        unavailable: impl Fn() -> E + Send,
        work: F,
    ) -> impl Future<Output = Result<T, E>> + Send
    where
        T: Send,
        E: Send,
        F: FnOnce(Self::Tx) -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send,
    {
        async move {
            let opened = self.pool.begin().await.map_err(|error| {
                tracing::error!(%error, "the control plane refused to open a transaction");
                unavailable()
            })?;
            let shared = SharedTx(Arc::new(Mutex::new(opened)));
            let outcome = work(shared.clone()).await;
            let Some(tx) = shared.into_transaction() else {
                tracing::error!("a repository outlived the transaction it was built from");
                return Err(unavailable());
            };
            match outcome {
                Ok(value) => {
                    tx.commit().await.map_err(|error| {
                        tracing::error!(%error, "the control plane refused to commit");
                        unavailable()
                    })?;
                    Ok(value)
                }
                Err(error) => {
                    if let Err(failure) = tx.rollback().await {
                        tracing::warn!(error = %failure, "a refused transaction did not roll back cleanly");
                    }
                    Err(error)
                }
            }
        }
    }
}
