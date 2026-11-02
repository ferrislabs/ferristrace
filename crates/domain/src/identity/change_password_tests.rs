use std::sync::Mutex;
use uuid::Uuid;

use super::change_password::ChangePassword;
use super::entities::Session;
use super::errors::{AuthError, ChangeError, HashError, ResetError, SessionError, WeakPassword};
use super::ports::{AuthProvider, PasswordHasher, ResetStore, SessionStore};
use super::value_objects::{
    AccountId, Agent, Email, Issuer, Password, PasswordCheck, PasswordHash, ResolvedIdentity,
    SignInProof, Subject,
};
use crate::clock::UnixSeconds;
use crate::secret::{SecretHash, SessionToken};
use crate::telemetry::TenantId;

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
