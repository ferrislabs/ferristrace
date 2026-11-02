use core::future::Future;
use std::sync::Arc;
use std::time::Duration;

use ferristrace_application::registry::{Settle, UnitOfWork};
use ferristrace_domain::SessionLifetime;
use ferristrace_infra::{Argon2Hasher, SecretSeal};
use sqlx::{PgPool, Transaction};
use tokio::sync::{Mutex, MutexGuard};

use crate::factors::DEFAULT_WINDOW as FACTOR_WINDOW;
use crate::flows::DEFAULT_WINDOW as FLOW_WINDOW;
use crate::provisioning::INVITE_WINDOW;
use crate::resets::DEFAULT_WINDOW as RESET_WINDOW;

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
    pub(crate) lifetime: SessionLifetime,
    pub(crate) hasher: Argon2Hasher,
    pub(crate) seal: Option<SecretSeal>,
    pub(crate) invite_window: Duration,
    pub(crate) reset_window: Duration,
    pub(crate) flow_window: Duration,
    pub(crate) factor_window: Duration,
}

impl<C> Postgres<C> {
    pub fn new(pool: PgPool, clock: C) -> Self {
        Self {
            pool,
            clock,
            lifetime: SessionLifetime::default(),
            hasher: Argon2Hasher::new().expect("the default argon2 cost is valid"),
            seal: None,
            invite_window: INVITE_WINDOW,
            reset_window: RESET_WINDOW,
            flow_window: FLOW_WINDOW,
            factor_window: FACTOR_WINDOW,
        }
    }

    #[must_use]
    pub fn with_session_lifetime(mut self, lifetime: SessionLifetime) -> Self {
        self.lifetime = lifetime;
        self
    }

    #[must_use]
    pub fn with_hasher(mut self, hasher: Argon2Hasher) -> Self {
        self.hasher = hasher;
        self
    }

    #[must_use]
    pub fn with_seal(mut self, seal: Option<SecretSeal>) -> Self {
        self.seal = seal;
        self
    }

    #[must_use]
    pub fn with_invite_window(mut self, window: Duration) -> Self {
        self.invite_window = window;
        self
    }

    #[must_use]
    pub fn with_reset_window(mut self, window: Duration) -> Self {
        self.reset_window = window;
        self
    }

    #[must_use]
    pub fn with_flow_window(mut self, window: Duration) -> Self {
        self.flow_window = window;
        self
    }

    #[must_use]
    pub fn with_factor_window(mut self, window: Duration) -> Self {
        self.factor_window = window;
        self
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

    #[allow(clippy::manual_async_fn)]
    fn transaction<T, E, F, Fut>(
        &self,
        unavailable: impl Fn() -> E + Send,
        settle: Settle,
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
                Err(error) if settle == Settle::Always => {
                    if let Err(failure) = tx.commit().await {
                        tracing::error!(error = %failure, "the control plane refused to keep a refused attempt");
                    }
                    Err(error)
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
