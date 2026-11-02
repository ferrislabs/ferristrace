use std::time::Duration;

use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, Clock, Email, PasswordHash, ResetError, ResetStore, SecretHash, UnixSeconds,
};
use sqlx::Row;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

const NANOS_PER_SECOND: i64 = 1_000_000_000;
pub(crate) const DEFAULT_WINDOW: Duration = Duration::from_secs(3600);

#[repository(domain = Resets, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresResets<C> {
    tx: SharedTx,
    clock: C,
    window: Duration,
}

impl<C: Clock> PostgresResets<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self
    where
        C: Clone,
    {
        Self {
            tx: tx.clone(),
            clock: backend.clock().clone(),
            window: backend.reset_window,
        }
    }

    #[must_use]
    pub fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    pub async fn prune(&self) -> Result<u64, ResetError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("DELETE FROM password_resets WHERE expires_at <= $1")
            .bind(to_nanos(self.clock.now()))
            .execute(&mut **tx)
            .await
            .map(|done| done.rows_affected())
            .map_err(|_| ResetError::StoreUnreachable)
    }
}

impl<C: Clock + Send + Sync> ResetStore for PostgresResets<C> {
    async fn account_for(&self, email: &Email) -> Result<Option<AccountId>, ResetError> {
        let mut tx = self.tx.lock().await;
        let found = sqlx::query("SELECT id AS id FROM accounts WHERE email = $1")
            .bind(email.as_str())
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| ResetError::StoreUnreachable)?;
        Ok(found.map(|row| AccountId::new(row.get("id"))))
    }

    async fn begin(&self, account: AccountId, digest: SecretHash) -> Result<(), ResetError> {
        let mut tx = self.tx.lock().await;
        let now = self.clock.now();
        sqlx::query(
            "INSERT INTO password_resets (digest, account_id, created_at, expires_at) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(digest.as_bytes().as_slice())
        .bind(account.as_uuid())
        .bind(to_nanos(now))
        .bind(to_nanos(now.saturating_add_secs(self.window.as_secs())))
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(|_| ResetError::StoreUnreachable)
    }

    async fn take(&self, digest: SecretHash) -> Result<AccountId, ResetError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "DELETE FROM password_resets WHERE digest = $1 AND expires_at > $2 \
             RETURNING account_id AS account_id",
        )
        .bind(digest.as_bytes().as_slice())
        .bind(to_nanos(self.clock.now()))
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ResetError::StoreUnreachable)?
        .ok_or(ResetError::Unknown)?;
        Ok(AccountId::new(row.get::<Uuid, _>("account_id")))
    }

    async fn set_password(
        &self,
        account: AccountId,
        hash: &PasswordHash,
    ) -> Result<(), ResetError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("UPDATE accounts SET password_hash = $2 WHERE id = $1")
            .bind(account.as_uuid())
            .bind(hash.encoded())
            .execute(&mut **tx)
            .await
            .map(|_| ())
            .map_err(|_| ResetError::StoreUnreachable)
    }

    async fn revoke_sessions(&self, account: AccountId) -> Result<u64, ResetError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "UPDATE sessions SET revoked_at = $2 \
             WHERE account_id = $1 AND revoked_at IS NULL",
        )
        .bind(account.as_uuid())
        .bind(to_nanos(self.clock.now()))
        .execute(&mut **tx)
        .await
        .map(|done| done.rows_affected())
        .map_err(|_| ResetError::StoreUnreachable)
    }
}

fn to_nanos(instant: UnixSeconds) -> i64 {
    i64::try_from(instant.get())
        .unwrap_or(i64::MAX / NANOS_PER_SECOND)
        .saturating_mul(NANOS_PER_SECOND)
}
