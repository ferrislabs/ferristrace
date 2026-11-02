use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use ferristrace_api::SessionCookie;
use ferristrace_api::reset_router;
use ferristrace_application::testing::Inert;
use ferristrace_domain::{
    AccountId, Email, HashError, Mailer, NoPhotos, Password, PasswordCheck, PasswordHash,
    PasswordHasher, ResetError, ResetStore, ResetToken, SecretHash,
};
use tower::ServiceExt;
use uuid::Uuid;

#[allow(dead_code)]
#[path = "support/identity.rs"]
mod identity;

use identity::{Fakes, Nowhere, state};

const TOKEN: &str = "ft_pr_0123456789";

#[derive(Clone)]
struct OnePending {
    live: Arc<Mutex<Vec<SecretHash>>>,
}

impl Default for OnePending {
    fn default() -> Self {
        Self {
            live: Arc::new(Mutex::new(vec![ResetToken::from_plaintext(TOKEN).hash()])),
        }
    }
}

impl OnePending {
    fn still_live(&self) -> usize {
        self.live.lock().expect("the lock").len()
    }
}

impl ResetStore for OnePending {
    async fn account_for(&self, _email: &Email) -> Result<Option<AccountId>, ResetError> {
        Ok(Some(AccountId::new(Uuid::nil())))
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
        Ok(AccountId::new(Uuid::nil()))
    }

    async fn set_password(
        &self,
        _account: AccountId,
        _hash: &PasswordHash,
    ) -> Result<(), ResetError> {
        Ok(())
    }

    async fn revoke_sessions(&self, _account: AccountId) -> Result<u64, ResetError> {
        Ok(0)
    }
}

struct Plain(PasswordHash);

impl Default for Plain {
    fn default() -> Self {
        Self(PasswordHash::from_encoded("decoy"))
    }
}

impl PasswordHasher for Plain {
    async fn hash(&self, password: &Password) -> Result<PasswordHash, HashError> {
        Ok(PasswordHash::from_encoded(password.exposed()))
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

impl ferristrace_domain::AlertMailer for Dropped {
    async fn send_alert(
        &self,
        _to: &ferristrace_domain::EmailList,
        _dispatch: &ferristrace_domain::Dispatch,
    ) -> Result<(), ferristrace_domain::DeliveryError> {
        Ok(())
    }

    async fn send_page(
        &self,
        _to: &Email,
        _dispatch: &ferristrace_domain::PageDispatch,
    ) -> Result<(), ferristrace_domain::DeliveryError> {
        Ok(())
    }
}

fn choosing(token: &str, password: &str) -> Request<Body> {
    Request::builder()
        .uri("/auth/reset/confirm")
        .method("POST")
        .header("x-requested-with", "fetch")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(format!(
            "{{\"token\":\"{token}\",\"password\":\"{password}\"}}"
        )))
        .expect("a request")
}

async fn served(pending: &OnePending, request: Request<Body>) -> (StatusCode, String) {
    let fakes = Fakes {
        sessions: Nowhere,
        memberships: Nowhere,
        accounts: Nowhere,
        gate: Nowhere,
        resets: pending.clone(),
        flows: Nowhere,
        local: Nowhere,
    };
    let reset = state(
        fakes,
        Plain::default(),
        Dropped,
        NoPhotos,
        None::<Inert>,
        SessionCookie::default(),
    );
    let response = reset_router(reset)
        .oneshot(request)
        .await
        .expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("text"))
}

#[tokio::test]
async fn a_password_below_the_floor_is_refused_with_the_rule_that_refused_it() {
    let pending = OnePending::default();

    let (status, body) = served(&pending, choosing(TOKEN, "short")).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, "a password needs at least 12 characters");
    assert_eq!(
        pending.still_live(),
        1,
        "the link must survive so the person can try again"
    );
}

#[tokio::test]
async fn a_long_enough_password_is_accepted_and_spends_the_link() {
    let pending = OnePending::default();

    let (status, _) = served(&pending, choosing(TOKEN, "a long enough one")).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(pending.still_live(), 0);
}

#[tokio::test]
async fn a_weak_password_and_a_bad_link_do_not_answer_alike() {
    let pending = OnePending::default();

    let (weak, _) = served(&pending, choosing(TOKEN, "short")).await;
    let (spent, _) = served(&pending, choosing("ft_pr_never", "a long enough one")).await;
    let (shaped, _) = served(&pending, choosing("garbage", "a long enough one")).await;

    assert_eq!(weak, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(spent, StatusCode::BAD_REQUEST);
    assert_eq!(
        shaped,
        StatusCode::BAD_REQUEST,
        "a token without the prefix is a bad link, not a weak password"
    );
}
