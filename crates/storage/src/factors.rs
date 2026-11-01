use std::time::Duration;

use ferristrace_domain::{
    AccountId, Clock, FactorError, FactorKind, FactorStore, Gate, Kept, Pending, SecretHash, Step,
    TotpSecret, UnixSeconds,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::seal::SecretSeal;

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const DEFAULT_WINDOW: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct PostgresFactors<C> {
    pool: PgPool,
    clock: C,
    seal: Option<SecretSeal>,
    window: Duration,
}

impl<C: Clock> PostgresFactors<C> {
    pub fn new(pool: PgPool, clock: C, seal: Option<SecretSeal>) -> Self {
        Self {
            pool,
            clock,
            seal,
            window: DEFAULT_WINDOW,
        }
    }

    #[must_use]
    pub fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    pub fn sealed(&self) -> bool {
        self.seal.is_some()
    }

    pub async fn prune(&self) -> Result<u64, FactorError> {
        sqlx::query("DELETE FROM second_step_challenges WHERE expires_at <= $1")
            .bind(to_nanos(self.clock.now()))
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| unreachable(&error))
    }

    fn key(&self) -> Result<&SecretSeal, FactorError> {
        self.seal.as_ref().ok_or_else(|| {
            tracing::error!(
                "a second factor was asked for but secrets.at_rest_key is not configured"
            );
            FactorError::NotOffered
        })
    }
}

impl<C: Clock + Send + Sync> FactorStore for PostgresFactors<C> {
    async fn live(&self, account: AccountId) -> Result<Vec<FactorKind>, FactorError> {
        let rows = sqlx::query(
            "SELECT kind AS kind FROM auth_factors \
             WHERE account_id = $1 AND confirmed_at IS NOT NULL ORDER BY created_at",
        )
        .bind(account.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| unreachable(&error))?;
        Ok(rows
            .iter()
            .filter_map(|row| FactorKind::parse(row.get::<&str, _>("kind")))
            .collect())
    }

    async fn keep(
        &self,
        account: AccountId,
        kind: FactorKind,
        secret: &TotpSecret,
    ) -> Result<(), FactorError> {
        let sealed = self.key()?.seal(secret).ok_or(FactorError::NotOffered)?;
        let done = sqlx::query(
            "INSERT INTO auth_factors (account_id, kind, secret, created_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (account_id, kind) DO UPDATE \
             SET secret = $3, created_at = $4, last_step = NULL \
             WHERE auth_factors.confirmed_at IS NULL",
        )
        .bind(account.as_uuid())
        .bind(kind.as_str())
        .bind(sealed.as_slice())
        .bind(to_nanos(self.clock.now()))
        .execute(&self.pool)
        .await
        .map_err(|error| unreachable(&error))?;
        if done.rows_affected() == 0 {
            return Err(FactorError::AlreadyKept);
        }
        Ok(())
    }

    async fn kept(
        &self,
        account: AccountId,
        kind: FactorKind,
    ) -> Result<Option<Kept>, FactorError> {
        let found = sqlx::query(
            "SELECT secret AS secret, confirmed_at AS confirmed_at, last_step AS last_step \
             FROM auth_factors WHERE account_id = $1 AND kind = $2",
        )
        .bind(account.as_uuid())
        .bind(kind.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| unreachable(&error))?;
        let Some(row) = found else {
            return Ok(None);
        };
        let sealed: Vec<u8> = row.get("secret");
        let Some(secret) = self.key()?.unseal(&sealed) else {
            tracing::error!(%account, "a sealed factor did not open under the configured key");
            return Err(FactorError::NotOffered);
        };
        Ok(Some(Kept {
            secret,
            confirmed: row.get::<Option<i64>, _>("confirmed_at").is_some(),
            last_step: row
                .get::<Option<i64>, _>("last_step")
                .map(|step| Step::new(u64::try_from(step).unwrap_or_default())),
        }))
    }

    async fn confirm(&self, account: AccountId, kind: FactorKind) -> Result<(), FactorError> {
        let done = sqlx::query(
            "UPDATE auth_factors SET confirmed_at = $3 \
             WHERE account_id = $1 AND kind = $2 AND confirmed_at IS NULL",
        )
        .bind(account.as_uuid())
        .bind(kind.as_str())
        .bind(to_nanos(self.clock.now()))
        .execute(&self.pool)
        .await
        .map_err(|error| unreachable(&error))?;
        if done.rows_affected() == 0 {
            return Err(FactorError::NoFactor);
        }
        Ok(())
    }

    async fn forget(&self, account: AccountId, kind: FactorKind) -> Result<bool, FactorError> {
        sqlx::query("DELETE FROM auth_factors WHERE account_id = $1 AND kind = $2")
            .bind(account.as_uuid())
            .bind(kind.as_str())
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|error| unreachable(&error))
    }

    async fn spent_step(
        &self,
        account: AccountId,
        kind: FactorKind,
        step: Step,
    ) -> Result<(), FactorError> {
        sqlx::query("UPDATE auth_factors SET last_step = $3 WHERE account_id = $1 AND kind = $2")
            .bind(account.as_uuid())
            .bind(kind.as_str())
            .bind(i64::try_from(step.get()).unwrap_or(i64::MAX))
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| unreachable(&error))
    }

    async fn keep_recovery(
        &self,
        account: AccountId,
        digests: &[SecretHash],
    ) -> Result<(), FactorError> {
        let now = to_nanos(self.clock.now());
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| unreachable(&error))?;
        sqlx::query("DELETE FROM recovery_codes WHERE account_id = $1")
            .bind(account.as_uuid())
            .execute(&mut *transaction)
            .await
            .map_err(|error| unreachable(&error))?;
        for digest in digests {
            sqlx::query(
                "INSERT INTO recovery_codes (digest, account_id, created_at) \
                 VALUES ($1, $2, $3) ON CONFLICT (digest) DO NOTHING",
            )
            .bind(digest.as_bytes().as_slice())
            .bind(account.as_uuid())
            .bind(now)
            .execute(&mut *transaction)
            .await
            .map_err(|error| unreachable(&error))?;
        }
        transaction
            .commit()
            .await
            .map_err(|error| unreachable(&error))
    }

    async fn spend_recovery(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> Result<bool, FactorError> {
        sqlx::query("DELETE FROM recovery_codes WHERE account_id = $1 AND digest = $2")
            .bind(account.as_uuid())
            .bind(digest.as_bytes().as_slice())
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|error| unreachable(&error))
    }

    async fn recovery_left(&self, account: AccountId) -> Result<u32, FactorError> {
        let row = sqlx::query("SELECT count(*) AS held FROM recovery_codes WHERE account_id = $1")
            .bind(account.as_uuid())
            .fetch_one(&self.pool)
            .await
            .map_err(|error| unreachable(&error))?;
        Ok(u32::try_from(row.get::<i64, _>("held")).unwrap_or(u32::MAX))
    }

    async fn answered(&self, digest: SecretHash) -> Result<Pending, FactorError> {
        let row = sqlx::query(
            "UPDATE second_step_challenges SET answers = answers + 1 \
             WHERE digest = $1 AND expires_at > $2 \
             RETURNING account_id AS account_id, answers AS answers",
        )
        .bind(digest.as_bytes().as_slice())
        .bind(to_nanos(self.clock.now()))
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| unreachable(&error))?
        .ok_or(FactorError::NoChallenge)?;
        Ok(Pending {
            account: AccountId::new(row.get::<Uuid, _>("account_id")),
            answers: u32::try_from(row.get::<i32, _>("answers")).unwrap_or(u32::MAX),
        })
    }

    async fn close(&self, digest: SecretHash) -> Result<(), FactorError> {
        sqlx::query("DELETE FROM second_step_challenges WHERE digest = $1")
            .bind(digest.as_bytes().as_slice())
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|error| unreachable(&error))
    }
}

impl<C: Clock + Send + Sync> Gate for PostgresFactors<C> {
    async fn offered(&self, account: AccountId) -> Result<Vec<FactorKind>, FactorError> {
        self.live(account).await
    }

    async fn open(&self, account: AccountId, digest: SecretHash) -> Result<(), FactorError> {
        let now = self.clock.now();
        sqlx::query(
            "INSERT INTO second_step_challenges (digest, account_id, created_at, expires_at) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(digest.as_bytes().as_slice())
        .bind(account.as_uuid())
        .bind(to_nanos(now))
        .bind(to_nanos(now.saturating_add_secs(self.window.as_secs())))
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|error| unreachable(&error))
    }
}

fn unreachable(error: &sqlx::Error) -> FactorError {
    tracing::error!(%error, "the control plane refused a second factor statement");
    FactorError::StoreUnreachable
}

fn to_nanos(instant: UnixSeconds) -> i64 {
    i64::try_from(instant.get())
        .unwrap_or(i64::MAX / NANOS_PER_SECOND)
        .saturating_mul(NANOS_PER_SECOND)
}
