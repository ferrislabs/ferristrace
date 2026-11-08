use ferristrace_application::repository;
use ferristrace_domain::{
    AccountDirectory, AccountId, AccountName, AccountTimezone, Accounts, AuthError, Email,
    PhotoKind, Portrait, Preferences, ProvisionPolicy, ResolvedIdentity, RowDensity, StartPage,
    StoreError, TimestampPrecision,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("the control plane refused the account query")]
    ControlPlane(#[from] sqlx::Error),
    #[error("this identity carries no account and provisioning is off")]
    NotProvisioned,
    #[error("an account already carries this address")]
    Taken,
}

#[repository(domain = Accounts, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresAccounts {
    tx: SharedTx,
}

impl PostgresAccounts {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }

    pub async fn create(&self, email: &Email) -> Result<AccountId, AccountError> {
        let mut tx = self.tx.lock().await;
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
            .bind(id)
            .bind(email.as_str())
            .execute(&mut **tx)
            .await
            .map_err(|error| match &error {
                sqlx::Error::Database(refusal) if refusal.is_unique_violation() => {
                    AccountError::Taken
                }
                _ => AccountError::ControlPlane(error),
            })?;
        Ok(AccountId::new(id))
    }

    pub async fn linked(
        &self,
        identity: &ResolvedIdentity,
    ) -> Result<Option<AccountId>, AccountError> {
        let mut tx = self.tx.lock().await;
        let found = sqlx::query(
            "SELECT account_id AS account_id FROM identity_links \
             WHERE issuer = $1 AND subject = $2",
        )
        .bind(identity.issuer.as_str())
        .bind(identity.subject.as_str())
        .fetch_optional(&mut **tx)
        .await?;
        Ok(found.map(|row| AccountId::new(row.get("account_id"))))
    }

    pub async fn by_email(&self, email: &Email) -> Result<Option<AccountId>, AccountError> {
        let mut tx = self.tx.lock().await;
        let found = sqlx::query("SELECT id AS id FROM accounts WHERE email = $1")
            .bind(email.as_str())
            .fetch_optional(&mut **tx)
            .await?;
        Ok(found.map(|row| AccountId::new(row.get("id"))))
    }

    pub async fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> Result<AccountId, AccountError> {
        if let Some(account) = self.linked(identity).await? {
            return Ok(account);
        }

        let mut tx = self.tx.lock().await;
        let mut kept = sqlx::Connection::begin(&mut **tx).await?;

        let existing: Option<Uuid> = if policy.link_by_email && identity.email_verified {
            sqlx::query("SELECT id AS id FROM accounts WHERE email = $1")
                .bind(identity.email.as_str())
                .fetch_optional(&mut *kept)
                .await?
                .map(|row| row.get("id"))
        } else {
            None
        };

        let account = match existing {
            Some(account) => account,
            None if policy.auto_provision => {
                let account = Uuid::new_v4();
                sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
                    .bind(account)
                    .bind(identity.email.as_str())
                    .execute(&mut *kept)
                    .await
                    .map_err(|error| match &error {
                        sqlx::Error::Database(refusal) if refusal.is_unique_violation() => {
                            AccountError::Taken
                        }
                        _ => AccountError::ControlPlane(error),
                    })?;
                account
            }
            None => return Err(AccountError::NotProvisioned),
        };

        sqlx::query("INSERT INTO identity_links (issuer, subject, account_id) VALUES ($1, $2, $3)")
            .bind(identity.issuer.as_str())
            .bind(identity.subject.as_str())
            .bind(account)
            .execute(&mut *kept)
            .await?;

        kept.commit().await?;

        Ok(AccountId::new(account))
    }
}

impl AccountDirectory for PostgresAccounts {
    async fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> Result<AccountId, AuthError> {
        match PostgresAccounts::provision(self, identity, policy).await {
            Ok(account) => Ok(account),
            Err(AccountError::NotProvisioned) => Err(AuthError::Rejected),
            Err(AccountError::Taken) => {
                tracing::warn!(
                    email = identity.email.as_str(),
                    "an unverified identity tried to claim an address someone else already holds"
                );
                Err(AuthError::Rejected)
            }
            Err(error) => {
                tracing::error!(%error, "the control plane refused to provision an account");
                Err(AuthError::ProviderUnreachable)
            }
        }
    }
}

impl Accounts for PostgresAccounts {
    async fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("UPDATE accounts SET name = $2 WHERE id = $1")
            .bind(account.as_uuid())
            .bind(name.map(AccountName::as_str))
            .execute(&mut **tx)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to rename an account");
                StoreError::Unreachable
            })?;
        Ok(())
    }

    async fn portrait_of(&self, account: AccountId) -> Result<Option<Portrait>, StoreError> {
        let mut tx = self.tx.lock().await;
        let found = sqlx::query(
            "SELECT photo_token AS photo_token, photo_type AS photo_type \
             FROM accounts WHERE id = $1",
        )
        .bind(account.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to read a photo");
            StoreError::Unreachable
        })?;
        Ok(found.as_ref().and_then(read_portrait))
    }

    async fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("UPDATE accounts SET photo_token = $2, photo_type = $3 WHERE id = $1")
            .bind(account.as_uuid())
            .bind(portrait.map(|kept| kept.token.as_ref()))
            .bind(portrait.map(|kept| kept.kind.as_str()))
            .execute(&mut **tx)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to keep a photo");
                StoreError::Unreachable
            })?;
        Ok(())
    }

    async fn preferences_of(&self, account: AccountId) -> Result<Option<Preferences>, StoreError> {
        let mut tx = self.tx.lock().await;
        let found = sqlx::query(
            "SELECT timezone, timestamp_precision, row_density, start_page \
             FROM accounts WHERE id = $1",
        )
        .bind(account.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to read preferences");
            StoreError::Unreachable
        })?;
        Ok(found.as_ref().map(read_preferences))
    }

    async fn set_preferences(
        &self,
        account: AccountId,
        preferences: &Preferences,
    ) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(
            "UPDATE accounts SET timezone = $2, timestamp_precision = $3, \
             row_density = $4, start_page = $5 WHERE id = $1",
        )
        .bind(account.as_uuid())
        .bind(preferences.timezone.as_str())
        .bind(preferences.precision.as_str())
        .bind(preferences.density.as_str())
        .bind(preferences.start_page.as_str())
        .execute(&mut **tx)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to keep preferences");
            StoreError::Unreachable
        })?;
        Ok(())
    }
}

fn read_preferences(row: &PgRow) -> Preferences {
    Preferences {
        timezone: row
            .get::<Option<String>, _>("timezone")
            .and_then(|held| AccountTimezone::try_from(held.as_str()).ok())
            .unwrap_or_default(),
        precision: row
            .get::<Option<String>, _>("timestamp_precision")
            .as_deref()
            .and_then(TimestampPrecision::parse)
            .unwrap_or_default(),
        density: row
            .get::<Option<String>, _>("row_density")
            .as_deref()
            .and_then(RowDensity::parse)
            .unwrap_or_default(),
        start_page: row
            .get::<Option<String>, _>("start_page")
            .as_deref()
            .and_then(StartPage::parse)
            .unwrap_or_default(),
    }
}

pub(crate) fn read_portrait(row: &PgRow) -> Option<Portrait> {
    let token: Option<String> = row.get("photo_token");
    let kind = row
        .get::<Option<String>, _>("photo_type")
        .as_deref()
        .and_then(PhotoKind::named);
    Some(Portrait {
        token: token?.into(),
        kind: kind?,
    })
}
