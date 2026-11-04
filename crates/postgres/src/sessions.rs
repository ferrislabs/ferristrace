use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, Email, ProvisionPolicy, ResolvedIdentity, Session, SessionError, SessionStore,
    SessionSwitch, SessionToken, TenantId,
};
use sqlx::Row;
use uuid::Uuid;

use crate::accounts::{AccountError, PostgresAccounts, read_portrait};
use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = Sessions, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresSessions {
    tx: SharedTx,
    accounts: PostgresAccounts,
    policy: ProvisionPolicy,
}

impl PostgresSessions {
    pub fn new<C>(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            accounts: PostgresAccounts::new(backend, tx),
            policy: backend.policy,
        }
    }

    async fn account_of(&self, identity: &ResolvedIdentity) -> Result<AccountId, SessionError> {
        match self.accounts.provision(identity, self.policy).await {
            Ok(account) => Ok(account),
            Err(AccountError::NotProvisioned) => Err(SessionError::Unknown),
            Err(error) => match self.accounts.linked(identity).await {
                Ok(Some(account)) => Ok(account),
                Ok(None) if matches!(error, AccountError::Taken) => Err(SessionError::Unknown),
                _ => {
                    tracing::error!(%error, "the control plane refused to provision an account");
                    Err(SessionError::StoreUnreachable)
                }
            },
        }
    }
}

impl SessionStore for PostgresSessions {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        let account = self.account_of(token.identity()).await?;
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "SELECT email AS email, name AS name, photo_token AS photo_token, \
                    photo_type AS photo_type, active_org_id AS active_org_id \
             FROM accounts WHERE id = $1",
        )
        .bind(account.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| SessionError::StoreUnreachable)?
        .ok_or(SessionError::Unknown)?;

        let email = Email::try_from(row.get::<String, _>("email").as_str())
            .map_err(|_| SessionError::StoreUnreachable)?;
        Ok(Session {
            account,
            email,
            name: row.get::<Option<String>, _>("name").map(Into::into),
            photo: read_portrait(&row),
            org: row
                .get::<Option<Uuid>, _>("active_org_id")
                .map(TenantId::new),
            expires_at: token.expires_at(),
        })
    }
}

impl SessionSwitch for PostgresSessions {
    async fn switch(&self, token: &SessionToken, org: TenantId) -> Result<(), SessionError> {
        let identity = token.identity();
        let mut tx = self.tx.lock().await;
        let switched = sqlx::query(
            "UPDATE accounts SET active_org_id = $3 \
             WHERE id IN (SELECT account_id FROM identity_links WHERE issuer = $1 AND subject = $2)",
        )
        .bind(identity.issuer.as_str())
        .bind(identity.subject.as_str())
        .bind(org.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(|_| SessionError::StoreUnreachable)?;
        if switched.rows_affected() == 0 {
            return Err(SessionError::Unknown);
        }
        Ok(())
    }
}
