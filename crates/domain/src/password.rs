use crate::credential::{Password, PasswordPolicy, WeakPassword};
use crate::identity::{AuthError, AuthProvider, SignInProof};
use crate::reset::{ResetError, ResetStore};
use crate::secret::SessionToken;
use crate::session::{HashError, PasswordHasher, SessionError, SessionStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChangeError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("that is not the current password")]
    NotTheCurrent,
    #[error(transparent)]
    Weak(#[from] WeakPassword),
    #[error("the password could not be changed")]
    Unavailable,
}

impl From<SessionError> for ChangeError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unknown => Self::NoSession,
            SessionError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl From<ResetError> for ChangeError {
    fn from(error: ResetError) -> Self {
        match error {
            ResetError::Weak(weak) => Self::Weak(weak),
            ResetError::Unknown
            | ResetError::StoreUnreachable
            | ResetError::Undeliverable
            | ResetError::Hashing(_) => Self::Unavailable,
        }
    }
}

impl From<HashError> for ChangeError {
    fn from(_: HashError) -> Self {
        Self::Unavailable
    }
}

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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use uuid::Uuid;

    use super::{ChangeError, ChangePassword};
    use crate::clock::UnixSeconds;
    use crate::credential::{Email, Issuer, Password, Subject, WeakPassword};
    use crate::identity::{AccountId, AuthError, AuthProvider, ResolvedIdentity, SignInProof};
    use crate::reset::{ResetError, ResetStore};
    use crate::secret::{SecretHash, SessionToken};
    use crate::session::{
        Agent, HashError, PasswordCheck, PasswordHash, PasswordHasher, Session, SessionError,
        SessionStore,
    };
    use crate::tenant::TenantId;

    const TOKEN: &str = "ct_ss_live";
    const CURRENT: &str = "the current one";

    fn an_account() -> AccountId {
        AccountId::new(Uuid::from_u128(9))
    }

    struct OneSession;

    impl SessionStore for OneSession {
        async fn issue(
            &self,
            _account: AccountId,
            _org: Option<TenantId>,
            _from: Option<&Agent>,
        ) -> Result<SessionToken, SessionError> {
            Ok(SessionToken::from_plaintext(TOKEN))
        }

        async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
            if token.exposed() == TOKEN {
                Ok(Session {
                    account: an_account(),
                    email: Email::try_from("person@example.com").expect("an address"),
                    name: None,
                    photo: None,
                    org: None,
                    expires_at: UnixSeconds::new(0),
                })
            } else {
                Err(SessionError::Unknown)
            }
        }

        async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
            Ok(())
        }
    }

    struct OnePassword;

    impl AuthProvider for OnePassword {
        async fn sign_in(&self, proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
            let SignInProof::Password { email, password } = proof else {
                return Err(AuthError::SourceDisabled);
            };
            if password.exposed() != CURRENT {
                return Err(AuthError::Rejected);
            }
            Ok(ResolvedIdentity {
                issuer: Issuer::Local,
                subject: Subject::try_from(an_account().to_string().as_str()).expect("a subject"),
                email,
                email_verified: true,
            })
        }
    }

    #[derive(Default)]
    struct Written {
        hashes: Mutex<Vec<PasswordHash>>,
        revoked: Mutex<Vec<AccountId>>,
    }

    impl ResetStore for &Written {
        async fn account_for(&self, _email: &Email) -> Result<Option<AccountId>, ResetError> {
            Ok(Some(an_account()))
        }

        async fn begin(&self, _account: AccountId, _digest: SecretHash) -> Result<(), ResetError> {
            Ok(())
        }

        async fn take(&self, _digest: SecretHash) -> Result<AccountId, ResetError> {
            Err(ResetError::Unknown)
        }

        async fn set_password(
            &self,
            _account: AccountId,
            hash: &PasswordHash,
        ) -> Result<(), ResetError> {
            self.hashes.lock().expect("the lock").push(hash.clone());
            Ok(())
        }

        async fn revoke_sessions(&self, account: AccountId) -> Result<u64, ResetError> {
            self.revoked.lock().expect("the lock").push(account);
            Ok(3)
        }
    }

    struct Reversing(PasswordHash);

    impl PasswordHasher for Reversing {
        async fn hash(&self, password: &Password) -> Result<PasswordHash, HashError> {
            Ok(PasswordHash::from_encoded(
                &password.exposed().chars().rev().collect::<String>(),
            ))
        }

        async fn check(
            &self,
            _password: &Password,
            _against: &PasswordHash,
        ) -> Result<PasswordCheck, HashError> {
            Ok(PasswordCheck::Rejected)
        }

        fn decoy(&self) -> &PasswordHash {
            &self.0
        }
    }

    fn changing(written: &Written) -> ChangePassword<OneSession, OnePassword, &Written, Reversing> {
        ChangePassword::new(
            OneSession,
            OnePassword,
            written,
            Reversing(PasswordHash::from_encoded("decoy")),
        )
    }

    #[tokio::test]
    async fn the_current_password_opens_the_change_and_every_session_then_goes() {
        let written = Written::default();

        let ended = changing(&written)
            .change(
                &SessionToken::from_plaintext(TOKEN),
                &Password::new(CURRENT),
                &Password::new("a long enough one"),
            )
            .await
            .expect("the change");

        assert_eq!(ended, 3);
        assert_eq!(
            written.hashes.lock().expect("the lock").len(),
            1,
            "the chosen password is written once"
        );
        assert_eq!(
            *written.revoked.lock().expect("the lock"),
            vec![an_account()]
        );
    }

    #[tokio::test]
    async fn a_wrong_current_password_writes_nothing() {
        let written = Written::default();

        let refused = changing(&written)
            .change(
                &SessionToken::from_plaintext(TOKEN),
                &Password::new("not the current one"),
                &Password::new("a long enough one"),
            )
            .await;

        assert_eq!(refused, Err(ChangeError::NotTheCurrent));
        assert!(written.hashes.lock().expect("the lock").is_empty());
        assert!(written.revoked.lock().expect("the lock").is_empty());
    }

    #[tokio::test]
    async fn a_weak_chosen_password_writes_nothing_either() {
        let written = Written::default();

        let refused = changing(&written)
            .change(
                &SessionToken::from_plaintext(TOKEN),
                &Password::new(CURRENT),
                &Password::new("short"),
            )
            .await;

        assert_eq!(refused, Err(ChangeError::Weak(WeakPassword::TooShort(12))));
        assert!(written.hashes.lock().expect("the lock").is_empty());
        assert!(written.revoked.lock().expect("the lock").is_empty());
    }

    #[tokio::test]
    async fn mistyping_the_current_password_is_not_reported_as_a_weak_new_one() {
        let written = Written::default();

        let refused = changing(&written)
            .change(
                &SessionToken::from_plaintext(TOKEN),
                &Password::new("not the current one"),
                &Password::new("short"),
            )
            .await;

        assert_eq!(
            refused,
            Err(ChangeError::NotTheCurrent),
            "the first thing wrong is the one to say"
        );
    }

    #[tokio::test]
    async fn a_token_no_session_carries_changes_nothing() {
        let written = Written::default();

        let refused = changing(&written)
            .change(
                &SessionToken::from_plaintext("ct_ss_nope"),
                &Password::new(CURRENT),
                &Password::new("a long enough one"),
            )
            .await;

        assert_eq!(refused, Err(ChangeError::NoSession));
        assert!(written.hashes.lock().expect("the lock").is_empty());
    }

    #[tokio::test]
    async fn the_chosen_password_reaches_the_store_hashed() {
        let written = Written::default();

        changing(&written)
            .change(
                &SessionToken::from_plaintext(TOKEN),
                &Password::new(CURRENT),
                &Password::new("a long enough one"),
            )
            .await
            .expect("the change");

        let hashes = written.hashes.lock().expect("the lock");
        assert_eq!(hashes[0].encoded(), "eno hguone gnol a");
    }
}
