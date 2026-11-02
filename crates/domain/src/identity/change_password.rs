use super::errors::{AuthError, ChangeError};
use super::ports::{AuthProvider, PasswordHasher, ResetStore, SessionStore};
use super::value_objects::{Password, PasswordPolicy, SignInProof};
use crate::secret::SessionToken;

#[derive(Debug)]
pub struct ChangePassword<S, L, R, H> {
    sessions: S,
    local: L,
    store: R,
    hasher: H,
    policy: PasswordPolicy,
}

impl<S, L, R, H> ChangePassword<S, L, R, H>
where
    S: SessionStore,
    L: AuthProvider,
    R: ResetStore,
    H: PasswordHasher,
{
    pub fn new(sessions: S, local: L, store: R, hasher: H) -> Self {
        Self {
            sessions,
            local,
            store,
            hasher,
            policy: PasswordPolicy::default(),
        }
    }

    #[must_use]
    pub fn with_policy(mut self, policy: PasswordPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub async fn change(
        &self,
        token: &SessionToken,
        current: &Password,
        chosen: &Password,
    ) -> Result<u64, ChangeError> {
        let session = self.sessions.resolve(token).await?;
        let proof = SignInProof::Password {
            email: session.email.clone(),
            password: current.clone(),
        };
        match self.local.sign_in(proof).await {
            Ok(_) => {}
            Err(AuthError::Rejected) => return Err(ChangeError::NotTheCurrent),
            Err(AuthError::SourceDisabled | AuthError::ProviderUnreachable) => {
                return Err(ChangeError::Unavailable);
            }
        }

        self.policy.admits(chosen)?;
        let hash = self.hasher.hash(chosen).await?;
        self.store.set_password(session.account, &hash).await?;
        Ok(self.store.revoke_sessions(session.account).await?)
    }
}
