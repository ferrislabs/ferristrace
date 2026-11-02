use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{Request, StatusCode};
use ferristrace_api::{SessionCookie, oidc_router};
use ferristrace_application::testing::Inert;
use ferristrace_domain::{
    AccountDirectory, AccountId, Agent, AuthError, AuthProvider, Authorizer, CodeVerifier,
    FlowError, Issuer, NoPhotos, OidcFlows, PendingFlow, ProvisionPolicy, ResolvedIdentity,
    Session, SessionError, SessionStore, SessionToken, SignInProof, Subject, TenantId,
};
use tower::ServiceExt;
use uuid::Uuid;

#[allow(dead_code)]
#[path = "support/identity.rs"]
mod identity;

use identity::{Directed, Fakes, Nowhere, state};

const AUTHORIZE: &str = "https://id.example/authorize?state=the-state";
const STATE: &str = "the-state";
const TOKEN: &str = "ft_ss_0123456789";
const LANDING: &str = "/console";

struct Provider {
    vouches: bool,
}

impl Authorizer for Provider {
    fn begin(&self) -> Result<(String, PendingFlow), AuthError> {
        Ok((
            AUTHORIZE.to_owned(),
            PendingFlow {
                state: STATE.to_owned(),
                nonce: "the-nonce".to_owned(),
                verifier: CodeVerifier::from_plaintext("the-verifier"),
                redirect_uri: "https://app.example.com/callback".to_owned(),
            },
        ))
    }
}

impl AuthProvider for Provider {
    async fn sign_in(&self, _proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        if !self.vouches {
            return Err(AuthError::Rejected);
        }
        Ok(ResolvedIdentity {
            issuer: Issuer::try_from("https://id.example").expect("a url"),
            subject: Subject::try_from("subject").expect("a subject"),
            email: ferristrace_domain::Email::try_from("person@example.com").expect("an address"),
            email_verified: true,
        })
    }
}

#[derive(Default)]
struct MemoryFlows {
    kept: Mutex<Vec<PendingFlow>>,
}

impl MemoryFlows {
    fn count(&self) -> usize {
        self.kept.lock().expect("the flow lock").len()
    }
}

impl OidcFlows for MemoryFlows {
    async fn begin(&self, flow: &PendingFlow) -> Result<(), FlowError> {
        self.kept.lock().expect("the flow lock").push(flow.clone());
        Ok(())
    }

    async fn take(&self, state: &str) -> Result<PendingFlow, FlowError> {
        let mut kept = self.kept.lock().expect("the flow lock");
        let found = kept.iter().position(|flow| flow.state == state);
        match found {
            Some(index) => Ok(kept.remove(index)),
            None => Err(FlowError::Unknown),
        }
    }
}

#[derive(Clone)]
struct OneAccount;

impl AccountDirectory for OneAccount {
    async fn provision(
        &self,
        _identity: &ResolvedIdentity,
        _policy: ProvisionPolicy,
    ) -> Result<AccountId, AuthError> {
        Ok(AccountId::new(Uuid::from_u128(7)))
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
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        self.live
            .lock()
            .expect("the session lock")
            .push(TOKEN.to_owned());
        Ok(SessionToken::from_plaintext(TOKEN))
    }

    async fn resolve(&self, _token: &SessionToken) -> Result<Session, SessionError> {
        Err(SessionError::Unknown)
    }

    async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
        Ok(())
    }
}

fn harness(vouches: bool) -> (axum::Router, Arc<MemoryFlows>, Arc<MemorySessions>) {
    let flows = Arc::new(MemoryFlows::default());
    let sessions = Arc::new(MemorySessions::default());
    let fakes = Fakes {
        sessions: Arc::clone(&sessions),
        memberships: Nowhere,
        accounts: Directed(OneAccount),
        flows: Arc::clone(&flows),
    };
    let app = state(
        fakes,
        Inert,
        NoPhotos,
        Some(Provider { vouches }),
        SessionCookie::default(),
    );
    let router = oidc_router(app, LANDING);
    (router, flows, sessions)
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("a request")
}

#[tokio::test]
async fn starting_sends_the_browser_to_the_provider_and_remembers_the_attempt() {
    let (router, flows, _sessions) = harness(true);

    let response = router
        .oneshot(get("/auth/oidc/start"))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(AUTHORIZE)
    );
    assert_eq!(flows.count(), 1);
}

#[tokio::test]
async fn a_callback_for_an_attempt_we_started_opens_a_session() {
    let (router, flows, sessions) = harness(true);
    router
        .clone()
        .oneshot(get("/auth/oidc/start"))
        .await
        .expect("the start");

    let response = router
        .oneshot(get(&format!("/auth/oidc/callback?code=abc&state={STATE}")))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(LANDING)
    );
    assert!(response.headers().get(SET_COOKIE).is_some());
    assert!(sessions.holds(TOKEN));
    assert_eq!(flows.count(), 0, "the attempt was spent");
}

#[tokio::test]
async fn a_callback_for_an_attempt_we_never_started_is_refused() {
    let (router, _flows, sessions) = harness(true);

    let response = router
        .oneshot(get("/auth/oidc/callback?code=abc&state=invented"))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.headers().get(SET_COOKIE).is_none());
    assert!(!sessions.holds(TOKEN));
}

#[tokio::test]
async fn a_replayed_callback_finds_its_attempt_spent() {
    let (router, _flows, sessions) = harness(true);
    router
        .clone()
        .oneshot(get("/auth/oidc/start"))
        .await
        .expect("the start");
    let uri = format!("/auth/oidc/callback?code=abc&state={STATE}");
    router
        .clone()
        .oneshot(get(&uri))
        .await
        .expect("the first callback");

    let replay = router.oneshot(get(&uri)).await.expect("the router answers");

    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    assert!(sessions.holds(TOKEN), "the first sign in stands");
}

#[tokio::test]
async fn a_provider_that_vouches_for_nobody_opens_nothing() {
    let (router, _flows, sessions) = harness(false);
    router
        .clone()
        .oneshot(get("/auth/oidc/start"))
        .await
        .expect("the start");

    let response = router
        .oneshot(get(&format!("/auth/oidc/callback?code=abc&state={STATE}")))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get(SET_COOKIE).is_none());
    assert!(!sessions.holds(TOKEN));
}
