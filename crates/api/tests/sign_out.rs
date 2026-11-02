use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{Request, StatusCode};
use ferristrace_api::{SessionCookie, sign_out_router};
use ferristrace_application::testing::Inert;
use ferristrace_domain::{
    AccountId, Agent, NoPhotos, Session, SessionError, SessionStore, SessionToken, TenantId,
    UnixSeconds,
};
use tower::ServiceExt;
use uuid::Uuid;

#[allow(dead_code)]
#[path = "support/identity.rs"]
mod identity;

use identity::{Fakes, Nowhere, state};

const TOKEN: &str = "ft_ss_0123456789";

#[derive(Default)]
struct MemorySessions {
    live: Mutex<Vec<String>>,
}

impl MemorySessions {
    fn seeded() -> Arc<Self> {
        Arc::new(Self {
            live: Mutex::new(vec![TOKEN.to_owned()]),
        })
    }

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
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Err(SessionError::StoreUnreachable)
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

fn cookie() -> SessionCookie {
    SessionCookie {
        secure: true,
        max_age_seconds: 600,
    }
}

fn router(sessions: Arc<MemorySessions>) -> axum::Router {
    let fakes = Fakes {
        sessions,
        memberships: Nowhere,
        accounts: Nowhere,
        flows: Nowhere,
    };
    sign_out_router(state(fakes, Inert, NoPhotos, None::<Inert>, cookie()))
}

fn sign_out_request(cookie: Option<&str>, with_header: bool) -> Request<Body> {
    let mut builder = Request::builder().method("POST").uri("/auth/sign-out");
    if with_header {
        builder = builder.header("x-requested-with", "ferristrace");
    }
    if let Some(cookie) = cookie {
        builder = builder.header(COOKIE, cookie);
    }
    builder.body(Body::empty()).expect("a request")
}

#[tokio::test]
async fn signing_out_revokes_the_session_and_clears_the_cookie() {
    let sessions = MemorySessions::seeded();
    assert!(sessions.holds(TOKEN));

    let response = router(Arc::clone(&sessions))
        .oneshot(sign_out_request(Some(&format!("ft_session={TOKEN}")), true))
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
    let sessions = MemorySessions::seeded();

    let response = router(sessions)
        .oneshot(sign_out_request(None, true))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response.headers().get(SET_COOKIE).is_some());
}

#[tokio::test]
async fn a_request_from_another_origin_is_refused_before_the_session_is_touched() {
    let sessions = MemorySessions::seeded();

    let response = router(Arc::clone(&sessions))
        .oneshot(sign_out_request(
            Some(&format!("ft_session={TOKEN}")),
            false,
        ))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(sessions.holds(TOKEN));
}

fn an_email() -> ferristrace_domain::Email {
    ferristrace_domain::Email::try_from("person@example.com").expect("an address")
}
