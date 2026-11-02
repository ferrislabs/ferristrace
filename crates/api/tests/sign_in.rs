use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{Request, StatusCode};
use ferristrace_api::{SessionCookie, auth_router};
use ferristrace_application::testing::Inert;
use ferristrace_domain::{
    AccountDirectory, AccountId, Agent, AuthError, AuthProvider, FactorError, FactorKind, Gate,
    Issuer, NoPhotos, NoSecondFactor, ProvisionPolicy, ResolvedIdentity, SecretHash, Session,
    SessionError, SessionStore, SessionToken, SignInProof, Subject, TenantId, UnixSeconds,
};
use tower::ServiceExt;
use uuid::Uuid;

#[allow(dead_code)]
#[path = "support/identity.rs"]
mod identity;

use identity::{Directed, Fakes, Nowhere, state};

const GOOD: &str = "correct horse";
const TOKEN: &str = "ct_ss_0123456789";

#[derive(Clone)]
struct OnePassword;

impl AuthProvider for OnePassword {
    async fn sign_in(&self, proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        let SignInProof::Password { email, password } = proof else {
            return Err(AuthError::SourceDisabled);
        };
        if password.exposed() != GOOD {
            return Err(AuthError::Rejected);
        }
        Ok(ResolvedIdentity {
            issuer: Issuer::Local,
            subject: Subject::try_from("subject").expect("a subject"),
            email,
            email_verified: true,
        })
    }
}

#[derive(Clone)]
struct OneAccount(AccountId);

impl AccountDirectory for OneAccount {
    async fn provision(
        &self,
        _identity: &ResolvedIdentity,
        _policy: ProvisionPolicy,
    ) -> Result<AccountId, AuthError> {
        Ok(self.0)
    }
}

#[derive(Default)]
struct MemorySessions {
    live: Mutex<Vec<String>>,
}

impl MemorySessions {
    fn holds(&self, token: &str) -> bool {
        self.live
            .lock()
            .expect("the session lock")
            .iter()
            .any(|held| held == token)
    }
}

impl SessionStore for MemorySessions {
    async fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        let _ = (account, org, from);
        self.live
            .lock()
            .expect("the session lock")
            .push(TOKEN.to_owned());
        Ok(SessionToken::from_plaintext(TOKEN))
    }

    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if self.holds(token.exposed()) {
            Ok(Session {
                account: AccountId::new(Uuid::nil()),
                email: an_email(),
                name: None,
                photo: None,
                org: None,
                expires_at: UnixSeconds::new(0),
            })
        } else {
            Err(SessionError::Unknown)
        }
    }

    async fn revoke(&self, token: &SessionToken) -> Result<(), SessionError> {
        self.live
            .lock()
            .expect("the session lock")
            .retain(|held| held != token.exposed());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct OneFactor {
    opened: Arc<Mutex<Vec<SecretHash>>>,
}

impl OneFactor {
    fn challenges(&self) -> usize {
        self.opened.lock().expect("the challenge lock").len()
    }
}

impl Gate for OneFactor {
    async fn offered(&self, _account: AccountId) -> Result<Vec<FactorKind>, FactorError> {
        Ok(vec![FactorKind::Authenticator])
    }

    async fn open(&self, _account: AccountId, digest: SecretHash) -> Result<(), FactorError> {
        self.opened.lock().expect("the challenge lock").push(digest);
        Ok(())
    }
}

fn cookie() -> SessionCookie {
    SessionCookie {
        secure: true,
        max_age_seconds: 600,
    }
}

fn routed<F>(sessions: &Arc<MemorySessions>, gate: F) -> axum::Router
where
    F: Gate + Clone + Send + Sync + 'static,
{
    let fakes = Fakes {
        sessions: Arc::clone(sessions),
        memberships: Nowhere,
        accounts: Directed(OneAccount(AccountId::new(Uuid::from_u128(7)))),
        gate,
        resets: Nowhere,
        flows: Nowhere,
        local: OnePassword,
    };
    auth_router(state(
        fakes,
        Inert::default(),
        Inert::default(),
        NoPhotos,
        None::<Inert>,
        cookie(),
    ))
}

fn harness() -> (axum::Router, Arc<MemorySessions>) {
    let sessions = Arc::new(MemorySessions::default());
    (routed(&sessions, NoSecondFactor), sessions)
}

fn guarded(gate: OneFactor) -> (axum::Router, Arc<MemorySessions>) {
    let sessions = Arc::new(MemorySessions::default());
    (routed(&sessions, gate), sessions)
}

fn sign_in_request(password: &str, with_header: bool) -> Request<Body> {
    let body = format!(r#"{{"email":"person@example.com","password":"{password}"}}"#);
    let mut builder = Request::builder()
        .method("POST")
        .uri("/auth/sign-in")
        .header(CONTENT_TYPE, "application/json");
    if with_header {
        builder = builder.header("x-requested-with", "comettrace");
    }
    builder.body(Body::from(body)).expect("a request")
}

#[tokio::test]
async fn the_right_password_opens_a_session_cookie() {
    let (router, sessions) = harness();

    let response = router
        .oneshot(sign_in_request(GOOD, true))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("a cookie");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Lax"), "{cookie}");
    assert!(cookie.contains("Secure"), "{cookie}");
    assert!(sessions.holds(TOKEN));
}

#[tokio::test]
async fn a_wrong_password_opens_nothing() {
    let (router, sessions) = harness();

    let response = router
        .oneshot(sign_in_request("wrong horse", true))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get(SET_COOKIE).is_none());
    assert!(!sessions.holds(TOKEN));
}

#[tokio::test]
async fn a_request_from_another_origin_is_refused_before_the_password_is_read() {
    let (router, sessions) = harness();

    let response = router
        .oneshot(sign_in_request(GOOD, false))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!sessions.holds(TOKEN));
}

#[tokio::test]
async fn signing_out_revokes_the_session_and_clears_the_cookie() {
    let (router, sessions) = harness();
    router
        .clone()
        .oneshot(sign_in_request(GOOD, true))
        .await
        .expect("the sign in");
    assert!(sessions.holds(TOKEN));

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/sign-out")
                .header("x-requested-with", "comettrace")
                .header(COOKIE, format!("ct_session={TOKEN}"))
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("a cookie");
    assert!(cookie.contains("Max-Age=0"), "{cookie}");
    assert!(!sessions.holds(TOKEN));
}

#[tokio::test]
async fn signing_out_without_a_cookie_still_clears_it() {
    let (router, _sessions) = harness();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/sign-out")
                .header("x-requested-with", "comettrace")
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response.headers().get(SET_COOKIE).is_some());
}

#[tokio::test]
async fn a_factor_turns_the_right_password_into_a_challenge_and_not_a_session() {
    let gate = OneFactor::default();
    let (router, sessions) = guarded(gate.clone());

    let response = router
        .oneshot(sign_in_request(GOOD, true))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("a cookie");
    assert!(cookie.starts_with("ct_second_step=ct_2f_"), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(!cookie.contains("ct_session="), "{cookie}");
    assert_eq!(gate.challenges(), 1);
    assert!(
        !sessions.holds(TOKEN),
        "no session is issued before the second step"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    assert_eq!(
        String::from_utf8(body.to_vec()).expect("text"),
        r#"{"second_step":["app"]}"#
    );
}

#[tokio::test]
async fn a_wrong_password_opens_no_challenge_either() {
    let gate = OneFactor::default();
    let (router, sessions) = guarded(gate.clone());

    let response = router
        .oneshot(sign_in_request("wrong horse", true))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get(SET_COOKIE).is_none());
    assert_eq!(gate.challenges(), 0);
    assert!(!sessions.holds(TOKEN));
}

fn an_email() -> ferristrace_domain::Email {
    ferristrace_domain::Email::try_from("person@example.com").expect("an address")
}
