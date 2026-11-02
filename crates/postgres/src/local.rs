use ferristrace_application::repository;
use ferristrace_domain::{
    AuthError, AuthProvider, Email, Issuer, Password, PasswordCheck, PasswordHash, PasswordHasher,
    ResolvedIdentity, SignInProof, Subject,
};
use ferristrace_infra::Argon2Hasher;
use sqlx::Row;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = LocalPasswords, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct LocalPasswordProvider {
    tx: SharedTx,
    hasher: Argon2Hasher,
}

struct Stored {
    account: Uuid,
    email: String,
    hash: Option<PasswordHash>,
}

impl LocalPasswordProvider {
    pub fn new<C>(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            hasher: backend.hasher.clone(),
        }
    }

    async fn stored(&self, email: &Email) -> Result<Option<Stored>, AuthError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "SELECT id AS id, email AS email, password_hash AS password_hash \
             FROM accounts WHERE email = $1",
        )
        .bind(email.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| AuthError::ProviderUnreachable)?;

        Ok(row.map(|row| Stored {
            account: row.get("id"),
            email: row.get("email"),
            hash: row
                .get::<Option<String>, _>("password_hash")
                .map(|encoded| PasswordHash::from_encoded(&encoded)),
        }))
    }

    async fn burn_the_same_time(&self, password: &Password) {
        let _ = self.hasher.check(password, self.hasher.decoy()).await;
    }

    async fn upgrade(&self, account: Uuid, hash: &PasswordHash) {
        let mut tx = self.tx.lock().await;
        let done = async {
            let mut kept = sqlx::Connection::begin(&mut **tx).await?;
            sqlx::query("UPDATE accounts SET password_hash = $2 WHERE id = $1")
                .bind(account)
                .bind(hash.encoded())
                .execute(&mut *kept)
                .await?;
            kept.commit().await
        }
        .await;
        if let Err(error) = done {
            tracing::error!(%error, "the password hash was not upgraded");
        }
    }
}

impl AuthProvider for LocalPasswordProvider {
    async fn sign_in(&self, proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        let SignInProof::Password { email, password } = proof else {
            return Err(AuthError::SourceDisabled);
        };

        let Some(stored) = self.stored(&email).await? else {
            self.burn_the_same_time(&password).await;
            return Err(AuthError::Rejected);
        };
        let Some(hash) = stored.hash else {
            self.burn_the_same_time(&password).await;
            return Err(AuthError::Rejected);
        };

        match self
            .hasher
            .check(&password, &hash)
            .await
            .map_err(|_| AuthError::Rejected)?
        {
            PasswordCheck::Rejected => return Err(AuthError::Rejected),
            PasswordCheck::Accepted => {}
            PasswordCheck::AcceptedBehindCost(upgraded) => {
                self.upgrade(stored.account, &upgraded).await;
            }
        }

        Ok(ResolvedIdentity {
            issuer: Issuer::Local,
            subject: Subject::try_from(stored.account.to_string().as_str())
                .map_err(|_| AuthError::Rejected)?,
            email: Email::try_from(stored.email.as_str()).map_err(|_| AuthError::Rejected)?,
            email_verified: true,
        })
    }
}
