use super::errors::ResetError;
use super::ports::{Mailer, PasswordHasher, ResetStore};
use super::value_objects::{Email, Password, PasswordPolicy};
use crate::secret::ResetToken;

#[derive(Debug)]
pub struct ResetPassword<R, H, M> {
    store: R,
    hasher: H,
    mailer: M,
    policy: PasswordPolicy,
}

impl<R, H, M> ResetPassword<R, H, M>
where
    R: ResetStore,
    H: PasswordHasher,
    M: Mailer,
{
    pub fn new(store: R, hasher: H, mailer: M) -> Self {
        Self {
            store,
            hasher,
            mailer,
            policy: PasswordPolicy::default(),
        }
    }

    #[must_use]
    pub fn with_policy(mut self, policy: PasswordPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub async fn request(&self, email: &Email, token: ResetToken) -> Result<bool, ResetError> {
        let Some(account) = self.store.account_for(email).await? else {
            return Ok(false);
        };
        self.store.begin(account, token.hash()).await?;
        self.mailer.send_password_reset(email, &token).await?;
        Ok(true)
    }

    pub async fn confirm(&self, token: &ResetToken, chosen: &Password) -> Result<u64, ResetError> {
        self.policy.admits(chosen)?;
        let account = self.store.take(token.hash()).await?;
        let hash = self.hasher.hash(chosen).await?;
        self.store.set_password(account, &hash).await?;
        self.store.revoke_sessions(account).await
    }
}
