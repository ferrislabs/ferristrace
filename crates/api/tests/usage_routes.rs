use std::sync::Arc;

use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, usage_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Agent, Consumed, DecompressedBytes, LiveSession, Membership, Memberships, NoPhotos,
    OrgRole, ProvisionPolicy, Session, SessionError, SessionId, SessionRoll, SessionStore,
    SessionToken, StoreError, TenantId, UnixSeconds, UsageHistory, UsageMeter, UsageReader,
    WhoamiError, Window,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const AN_HOUR_MS: u64 = 3_600_000;

fn id(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
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
                account: AccountId::new(Uuid::from_u128(9)),
                email: "person@example.com".try_into().expect("an address"),
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

impl SessionRoll for OneSession {
    async fn live(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<Vec<LiveSession>, SessionError> {
        Ok(Vec::new())
    }

    async fn revoke_one(&self, _account: AccountId, _id: SessionId) -> Result<bool, SessionError> {
        Ok(false)
    }

    async fn revoke_others(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<u64, SessionError> {
        Ok(0)
    }
}

struct MemberOfMine;

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: TenantId::new(Uuid::from_u128(MINE)),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

struct Live(u64);

impl UsageMeter for Live {
    async fn record_ingest(&self, _tenant: TenantId, _decompressed: DecompressedBytes) {}
}

impl UsageReader for Live {
    async fn used(&self, _tenant: TenantId) -> DecompressedBytes {
        DecompressedBytes::new(self.0)
    }
}

#[derive(Debug)]
struct Rolled {
    included: Option<u64>,
}

impl UsageHistory for Rolled {
    async fn hours(&self, _org: TenantId, window: Window) -> Result<Vec<Consumed>, StoreError> {
        Ok(vec![Consumed {
            started_at_unix_nanos: window.from_unix_nanos(),
            ingested_bytes: 4_096,
        }])
    }

    async fn included(&self, _org: TenantId) -> Result<Option<DecompressedBytes>, StoreError> {
        Ok(self.included.map(DecompressedBytes::new))
    }
}

fn asking(org: u128, query: &str, session: Option<&str>, scripted: bool) -> Request<Body> {
    let mut request = Request::builder().uri(format!("/api/orgs/{}/usage?{query}", id(org)));
    if let Some(token) = session {
        request = request.header(COOKIE, format!("ft_session={token}"));
    }
    if scripted {
        request = request.header("x-requested-with", "fetch");
    }
    request.body(Body::empty()).expect("a request")
}

struct Fakes {
    included: Option<u64>,
}

impl Untransacted for Fakes {}

impl RepoFor<domain::Sessions> for Fakes {
    type Repo = OneSession;

    fn build(&self, (): &()) -> Self::Repo {
        OneSession
    }
}

impl RepoFor<domain::Memberships> for Fakes {
    type Repo = MemberOfMine;

    fn build(&self, (): &()) -> Self::Repo {
        MemberOfMine
    }
}

impl RepoFor<domain::UsageHistory> for Fakes {
    type Repo = Rolled;

    fn build(&self, (): &()) -> Self::Repo {
        Rolled {
            included: self.included,
        }
    }
}

struct TestStack;

impl Stack for TestStack {
    type Repos = Fakes;
    type Clock = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Live;
    type Oidc = Inert;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

fn state(live: u64, included: Option<u64>) -> AppState<TestStack> {
    let fakes = Fakes { included };
    let adapters = Adapters {
        clock: Inert,
        mailer: Inert,
        webhooks: Inert,
        photos: NoPhotos,
        meter: Live(live),
        span_store: None,
        issue_store: None,
        log_store: None,
        metric_store: None,
        oidc: None,
        provision: ProvisionPolicy::default(),
    };
    AppState::new(
        Arc::new(Ferristrace::new(fakes, adapters)),
        SessionCookie::default(),
    )
}

async fn served(live: u64, included: Option<u64>, request: Request<Body>) -> (StatusCode, String) {
    let response = usage_router(state(live, included))
        .oneshot(request)
        .await
        .expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("text"))
}

fn read(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("json")
}

fn a_month() -> String {
    format!("from={AN_HOUR_MS}&to={}", AN_HOUR_MS * 800)
}

#[tokio::test]
async fn a_member_reads_what_its_organisation_ingested() {
    let (status, body) = served(
        9_999,
        Some(1_073_741_824),
        asking(MINE, &a_month(), Some(TOKEN), true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let metered = read(&body);
    assert_eq!(metered["ingested_bytes"], 9_999);
    assert_eq!(metered["included_bytes"], 1_073_741_824u64);
    assert_eq!(metered["hours"][0]["ingested_bytes"], 4_096);
    assert_eq!(
        metered["hours"][0]["started_at_ms"], AN_HOUR_MS,
        "a window asked for in milliseconds comes back in milliseconds"
    );
}

#[tokio::test]
async fn a_plan_with_no_ceiling_answers_null_rather_than_zero() {
    let (status, body) = served(9_999, None, asking(MINE, &a_month(), Some(TOKEN), true)).await;

    assert_eq!(status, StatusCode::OK);
    assert!(read(&body)["included_bytes"].is_null());
}

#[tokio::test]
async fn an_organisation_this_session_does_not_belong_to_is_not_found() {
    let (status, _) = served(0, None, asking(THEIRS, &a_month(), Some(TOKEN), true)).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
}

#[tokio::test]
async fn a_window_that_ends_before_it_starts_is_refused() {
    let (status, body) = served(
        0,
        None,
        asking(MINE, "from=2000&to=1000", Some(TOKEN), true),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a malformed query string, which is what the trace routes answer for the same window"
    );
    assert!(body.contains("window"), "{body}");
}

#[tokio::test]
async fn a_request_carrying_no_session_is_told_to_sign_in() {
    let (status, _) = served(0, None, asking(MINE, &a_month(), None, true)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_request_from_another_origin_never_reaches_the_usage() {
    let (status, _) = served(0, None, asking(MINE, &a_month(), Some(TOKEN), false)).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}
