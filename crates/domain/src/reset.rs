use core::future::Future;

use crate::credential::{Email, Password, PasswordPolicy, WeakPassword};
use crate::identity::AccountId;
use crate::secret::{ResetToken, SecretHash};
use crate::session::{HashError, PasswordHash, PasswordHasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ResetError {
    #[error("no live reset carries this token")]
    Unknown,
    #[error("the control plane could not be reached")]
    StoreUnreachable,
    #[error("the message could not be sent")]
    Undeliverable,
    #[error(transparent)]
    Hashing(#[from] HashError),
    #[error(transparent)]
    Weak(#[from] WeakPassword),
}

pub trait ResetStore {
    fn account_for(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<Option<AccountId>, ResetError>> + Send;

    fn begin(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<(), ResetError>> + Send;

    fn take(
        &self,
        digest: SecretHash,
    ) -> impl Future<Output = Result<AccountId, ResetError>> + Send;

    fn set_password(
        &self,
        account: AccountId,
        hash: &PasswordHash,
    ) -> impl Future<Output = Result<(), ResetError>> + Send;

    fn revoke_sessions(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<u64, ResetError>> + Send;
}

pub trait Mailer {
    fn send_password_reset(
        &self,
        to: &Email,
        token: &ResetToken,
    ) -> impl Future<Output = Result<(), ResetError>> + Send;
}

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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{Mailer, ResetError, ResetPassword, ResetStore};
    use crate::credential::{Email, Password, PasswordPolicy, WeakPassword};
    use crate::identity::AccountId;
    use crate::secret::{ResetToken, SecretHash};
    use crate::session::{HashError, PasswordCheck, PasswordHash, PasswordHasher};

    #[derive(Default)]
    struct Pending {
        live: Mutex<Vec<SecretHash>>,
        chosen: Mutex<Option<PasswordHash>>,
    }

    impl Pending {
        fn holding(digest: SecretHash) -> Self {
            Self {
                live: Mutex::new(vec![digest]),
                chosen: Mutex::new(None),
            }
        }

        fn still_live(&self) -> usize {
            self.live.lock().expect("the lock").len()
        }
    }

    impl ResetStore for &Pending {
        async fn account_for(&self, _email: &Email) -> Result<Option<AccountId>, ResetError> {
            Ok(Some(an_account()))
        }

        async fn begin(&self, _account: AccountId, digest: SecretHash) -> Result<(), ResetError> {
            self.live.lock().expect("the lock").push(digest);
            Ok(())
        }

        async fn take(&self, digest: SecretHash) -> Result<AccountId, ResetError> {
            let mut live = self.live.lock().expect("the lock");
            let Some(at) = live.iter().position(|held| *held == digest) else {
                return Err(ResetError::Unknown);
            };
            live.remove(at);
            Ok(an_account())
        }

        async fn set_password(
            &self,
            _account: AccountId,
            hash: &PasswordHash,
        ) -> Result<(), ResetError> {
            *self.chosen.lock().expect("the lock") = Some(hash.clone());
            Ok(())
        }

        async fn revoke_sessions(&self, _account: AccountId) -> Result<u64, ResetError> {
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

    struct Dropped;

    impl Mailer for Dropped {
        async fn send_password_reset(
            &self,
            _to: &Email,
            _token: &ResetToken,
        ) -> Result<(), ResetError> {
            Ok(())
        }
    }

    fn an_account() -> AccountId {
        AccountId::new(uuid::Uuid::from_u128(1))
    }

    fn hasher() -> Reversing {
        Reversing(PasswordHash::from_encoded("decoy"))
    }

    #[tokio::test]
    async fn a_refused_password_does_not_spend_the_token() {
        let token = ResetToken::from_plaintext("ct_pr_one");
        let store = Pending::holding(token.hash());
        let reset = ResetPassword::new(&store, hasher(), Dropped);

        let refused = reset.confirm(&token, &Password::new("short")).await;

        assert_eq!(refused, Err(ResetError::Weak(WeakPassword::TooShort(12))));
        assert_eq!(
            store.still_live(),
            1,
            "the person must be able to try again with a longer one"
        );
        assert!(store.chosen.lock().expect("the lock").is_none());

        let ended = reset
            .confirm(&token, &Password::new("a long enough one"))
            .await
            .expect("the second attempt");

        assert_eq!(ended, 3);
        assert_eq!(store.still_live(), 0, "the token is spent once it works");
    }

    #[tokio::test]
    async fn the_policy_is_asked_before_the_store_is_touched() {
        let store = Pending::default();
        let reset = ResetPassword::new(&store, hasher(), Dropped);
        let unknown = ResetToken::from_plaintext("ct_pr_never_issued");

        let refused = reset.confirm(&unknown, &Password::new("short")).await;

        assert_eq!(
            refused,
            Err(ResetError::Weak(WeakPassword::TooShort(12))),
            "a weak password is refused on its own terms, not as a bad link"
        );
    }

    #[tokio::test]
    async fn a_chosen_password_reaches_the_store_hashed() {
        let token = ResetToken::from_plaintext("ct_pr_one");
        let store = Pending::holding(token.hash());
        let reset = ResetPassword::new(&store, hasher(), Dropped);

        reset
            .confirm(&token, &Password::new("a long enough one"))
            .await
            .expect("the confirmation");

        let stored = store.chosen.lock().expect("the lock").clone();
        assert_eq!(
            stored.as_ref().map(PasswordHash::encoded),
            Some("eno hguone gnol a"),
            "the plaintext must never reach the store"
        );
    }

    #[tokio::test]
    async fn a_tightened_policy_is_the_one_that_is_asked() {
        let token = ResetToken::from_plaintext("ct_pr_one");
        let store = Pending::holding(token.hash());
        let reset = ResetPassword::new(&store, hasher(), Dropped).with_policy(PasswordPolicy {
            min_chars: 30,
            max_bytes: 128,
        });

        let refused = reset
            .confirm(&token, &Password::new("a long enough one"))
            .await;

        assert_eq!(refused, Err(ResetError::Weak(WeakPassword::TooShort(30))));
    }
}
