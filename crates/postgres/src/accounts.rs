use ferristrace_application::repository;
use ferristrace_domain::{
    AccountDirectory, AccountId, AccountName, Accounts, AuthError, Email, PasswordHash, PhotoKind,
    Portrait, ProvisionPolicy, ResolvedIdentity, StoreError,
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

    pub async fn create(
        &self,
        email: &Email,
        locked: &PasswordHash,
    ) -> Result<AccountId, AccountError> {
        let mut tx = self.tx.lock().await;
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(email.as_str())
            .bind(locked.encoded())
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

    pub async fn established(
        &self,
        identity: &ResolvedIdentity,
    ) -> Result<AccountId, AccountError> {
        let mut tx = self.tx.lock().await;
        let id =
            Uuid::parse_str(identity.subject.as_str()).map_err(|_| AccountError::NotProvisioned)?;
        let found = sqlx::query("SELECT id AS id FROM accounts WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
        found
            .map(|row| AccountId::new(row.get("id")))
            .ok_or(AccountError::NotProvisioned)
    }

    pub async fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> Result<AccountId, AccountError> {
        if identity.issuer.is_local() {
            return self.established(identity).await;
        }
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
