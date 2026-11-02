use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, explore_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Agent, LiveSession, Membership, Memberships, NoPhotos, OrgRole, ProjectId,
    ProvisionPolicy, ServiceSummary, Session, SessionError, SessionId, SessionRoll, SessionStore,
    SessionToken, Span, SpanCursor, SpanEvent, SpanId, SpanKind, SpanPage, SpanPages, SpanReader,
    SpanStatus, StoreError, TenantId, TraceFilter, TraceId, TraceOrder, TraceSummary, UnixSeconds,
    WhoamiError, Window,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const TRACE: &str = "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a";

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

struct TwoSpans;

fn a_trace_id() -> TraceId {
    TraceId::try_from(hex::decode(TRACE).expect("hex").as_slice()).expect("sixteen bytes")
}

fn a_span(seed: u8, parent: Option<u8>, start: u64, end: u64) -> Span {
    Span {
        trace_id: a_trace_id(),
        span_id: SpanId::try_from([seed; 8].as_slice()).expect("eight bytes"),
        parent: parent.map(|id| SpanId::try_from([id; 8].as_slice()).expect("eight bytes")),
        name: "work".into(),
        service_name: "checkout".into(),
        kind: SpanKind::Server,
        status: SpanStatus::Ok,
        start_unix_nanos: start,
        end_unix_nanos: end,
        attributes: BTreeMap::from([("http.status_code".into(), "200".into())]),
        events: Vec::new(),
    }
}

fn with_an_exception(mut span: Span, at: u64) -> Span {
    span.events = vec![SpanEvent {
        time_unix_nanos: at,
        name: "exception".into(),
        attributes: BTreeMap::from([
            ("exception.type".into(), "PspTimeoutError".into()),
            (
                "exception.message".into(),
                "deadline exceeded after 960ms".into(),
            ),
        ]),
    }];
    span
}

impl SpanPages for TwoSpans {
    async fn page(
        &self,
        _org: TenantId,
        _after: Option<&SpanCursor>,
        _limit: u32,
    ) -> Result<SpanPage, StoreError> {
        Ok(SpanPage::default())
    }
}

impl SpanReader for TwoSpans {
    async fn services(
        &self,
        _tenant: TenantId,
        _window: Window,
        _project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(vec![ServiceSummary {
            name: "checkout".into(),
            requests: 10,
            errors: 1,
            p50_nanos: 96_000_000,
            p95_nanos: 1_280_000_000,
            p99_nanos: 2_900_000_000,
            last_seen_unix_nanos: 1_700_000_000_123_456_789,
        }])
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        _filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        Ok(vec![TraceSummary {
            trace_id: a_trace_id(),
            root_service: "checkout".into(),
            root_name: "pay".into(),
            started_unix_nanos: 1_700_000_000_123_456_789,
            duration_nanos: 2_500_000,
            spans: 2,
            errors: 1,
        }])
    }

    async fn trace(
        &self,
        _tenant: TenantId,
        _id: TraceId,
        _project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        Ok(vec![
            a_span(
                1,
                None,
                1_700_000_000_000_000_000,
                1_700_000_002_000_000_000,
            ),
            with_an_exception(
                a_span(
                    2,
                    Some(1),
                    1_700_000_000_500_000_000,
                    1_700_000_001_000_000_000,
                ),
                1_700_000_000_900_000_000,
            ),
        ])
    }
}

struct NoSpans;

impl SpanPages for NoSpans {
    async fn page(
        &self,
        _org: TenantId,
        _after: Option<&SpanCursor>,
        _limit: u32,
    ) -> Result<SpanPage, StoreError> {
        Ok(SpanPage::default())
    }
}

impl SpanReader for NoSpans {
    async fn services(
        &self,
        _tenant: TenantId,
        _window: Window,
        _project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        _filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn trace(
        &self,
        _tenant: TenantId,
        _id: TraceId,
        _project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        Ok(Vec::new())
    }
}

fn looking(uri: &str, cookie: Option<&str>, scripted: bool) -> Request<Body> {
    let mut request = Request::builder().uri(uri);
    if scripted {
        request = request.header("x-requested-with", "fetch");
    }
    if let Some(cookie) = cookie {
        request = request.header(COOKIE, cookie);
    }
    request.body(Body::empty()).expect("a request")
}

struct Fakes;

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

impl RepoFor<domain::ApiKeys> for Fakes {
    type Repo = Inert;

    fn build(&self, (): &()) -> Self::Repo {
        Inert::default()
    }
}

struct TestStack<R>(std::marker::PhantomData<R>);

impl<R: SpanReader + SpanPages + Send + Sync + 'static> Stack for TestStack<R> {
    type Repos = Fakes;
    type Clock = Inert;
    type Hasher = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = R;
    type IssueStore = Inert;
    type Meter = Inert;
    type Oidc = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
}

fn state<R: SpanReader + SpanPages + Send + Sync + 'static>(reader: R) -> AppState<TestStack<R>> {
    let adapters = Adapters {
        clock: Inert::default(),
        hasher: Inert::default(),
        mailer: Inert::default(),
        photos: NoPhotos,
        meter: Inert::default(),
        span_store: Some(reader),
        issue_store: None,
        oidc: None,
        provision: ProvisionPolicy::default(),
    };
    AppState::new(
        Arc::new(Ferristrace::new(Fakes, adapters)),
        SessionCookie::default(),
    )
}

async fn served<R>(reader: R, request: Request<Body>) -> (StatusCode, String)
where
    R: SpanReader + SpanPages + Send + Sync + 'static,
{
    let response = explore_router(state(reader))
        .oneshot(request)
        .await
        .expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("text"))
}

fn org(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

fn session() -> String {
    format!("ft_session={TOKEN}")
}

#[tokio::test]
async fn a_member_lists_the_traces_of_its_own_organisation() {
    let uri = format!(
        "/api/orgs/{}/traces?from=1700000000000&to=1700000100000",
        org(MINE)
    );

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::OK);
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    let listed = &found["traces"][0];
    assert_eq!(listed["trace_id"], TRACE);
    assert_eq!(listed["root_service"], "checkout");
    assert_eq!(listed["spans"], 2);
    assert_eq!(listed["errors"], 1);
    assert_eq!(
        listed["started_at_ms"], 1_700_000_000_123u64,
        "a time a browser can hold without losing digits"
    );
    assert_eq!(listed["duration_us"], 2500);
}

#[tokio::test]
async fn an_organisation_this_session_does_not_belong_to_is_not_found() {
    let uri = format!("/api/orgs/{}/traces?from=1&to=2", org(THEIRS));

    let (status, _) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
}

#[tokio::test]
async fn a_request_without_a_session_is_refused() {
    let uri = format!("/api/orgs/{}/traces?from=1&to=2", org(MINE));

    let (no_cookie, _) = served(TwoSpans, looking(&uri, None, true)).await;
    let (unknown, _) = served(TwoSpans, looking(&uri, Some("ft_session=ft_ss_nope"), true)).await;
    let (unscripted, _) = served(TwoSpans, looking(&uri, Some(&session()), false)).await;

    assert_eq!(no_cookie, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown, StatusCode::UNAUTHORIZED);
    assert_eq!(unscripted, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_window_that_ends_before_it_starts_is_refused() {
    let uri = format!("/api/orgs/{}/traces?from=20&to=10", org(MINE));

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("must end after it starts"), "{body}");
}

#[tokio::test]
async fn a_window_nobody_gave_is_refused() {
    let uri = format!("/api/orgs/{}/traces", org(MINE));

    let (status, _) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn one_trace_is_laid_out_for_a_waterfall() {
    let uri = format!("/api/orgs/{}/traces/{TRACE}", org(MINE));

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::OK);
    let trace: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(trace["trace_id"], TRACE);
    assert_eq!(trace["duration_us"], 2_000_000);
    let root = &trace["spans"][0];
    assert_eq!(root["start_offset_us"], 0, "the root anchors the waterfall");
    assert_eq!(root["duration_us"], 2_000_000);
    assert_eq!(root["parent_span_id"], serde_json::Value::Null);
    assert_eq!(root["kind"], "server");
    assert_eq!(root["status"], "ok");
    assert_eq!(root["attributes"]["http.status_code"], "200");
    let child = &trace["spans"][1];
    assert_eq!(child["start_offset_us"], 500_000);
    assert_eq!(child["duration_us"], 500_000);
    assert_eq!(child["parent_span_id"], "0101010101010101");
}

#[tokio::test]
async fn a_trace_identifier_that_is_not_one_is_refused() {
    for candidate in ["nothex", "0a0a", "00000000000000000000000000000000"] {
        let uri = format!("/api/orgs/{}/traces/{candidate}", org(MINE));

        let (status, _) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{candidate} should not reach the store"
        );
    }
}

#[tokio::test]
async fn a_trace_the_store_does_not_hold_is_not_found() {
    let uri = format!("/api/orgs/{}/traces/{TRACE}", org(MINE));

    let (status, _) = served(NoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_empty_list_is_an_answer_rather_than_an_error() {
    let uri = format!("/api/orgs/{}/traces?from=1&to=2", org(MINE));

    let (status, body) = served(NoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"traces":[]}"#);
}

fn an_email() -> ferristrace_domain::Email {
    ferristrace_domain::Email::try_from("person@example.com").expect("an address")
}

#[tokio::test]
async fn the_services_table_is_read_for_the_same_organisation() {
    let uri = format!(
        "/api/orgs/{}/services?from=1700000000000&to=1700000060000",
        org(MINE)
    );

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::OK);
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    let service = &found["services"][0];
    assert_eq!(service["name"], "checkout");
    assert_eq!(service["requests"], 10);
    assert_eq!(service["errors"], 1);
    assert_eq!(
        service["per_second"], 0.166_666_666_666_666_66,
        "ten requests over a minute"
    );
    assert_eq!(service["p50_us"], 96_000);
    assert_eq!(service["p99_us"], 2_900_000);
    assert_eq!(service["last_seen_ms"], 1_700_000_000_123u64);
}

#[tokio::test]
async fn the_services_of_an_organisation_this_session_does_not_hold_are_not_found() {
    let uri = format!("/api/orgs/{}/services?from=1&to=2", org(THEIRS));

    let (status, _) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_services_window_that_ends_before_it_starts_is_refused() {
    let uri = format!("/api/orgs/{}/services?from=20&to=10", org(MINE));

    let (status, _) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_exception_on_a_span_reaches_the_waterfall_with_its_place_in_it() {
    let uri = format!("/api/orgs/{}/traces/{TRACE}", org(MINE));

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::OK);
    let trace: serde_json::Value = serde_json::from_str(&body).expect("json");
    let child = &trace["spans"][1];
    let events = child["events"].as_array().expect("the events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["name"], "exception");
    assert_eq!(events[0]["offset_us"], 900_000);
    assert_eq!(events[0]["attributes"]["exception.type"], "PspTimeoutError");
    assert_eq!(
        trace["spans"][0]["events"].as_array().map(Vec::len),
        Some(0)
    );
}

#[derive(Debug, Default)]
struct Remembering(Mutex<Option<TraceFilter>>);

impl SpanPages for Remembering {
    async fn page(
        &self,
        _org: TenantId,
        _after: Option<&SpanCursor>,
        _limit: u32,
    ) -> Result<SpanPage, StoreError> {
        Ok(SpanPage::default())
    }
}

impl SpanReader for Remembering {
    async fn services(
        &self,
        _tenant: TenantId,
        _window: Window,
        _project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        *self.0.lock().expect("the lock") = Some(filter.clone());
        Ok(Vec::new())
    }

    async fn trace(
        &self,
        _tenant: TenantId,
        _id: TraceId,
        _project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn every_filter_the_screen_offers_reaches_the_store() {
    let asked = Arc::new(Remembering::default());
    let uri = format!(
        "/api/orgs/{}/traces?from=1000&to=2000&service=payments&name=psp.charge\
         &errors=true&at_least_us=1500&order=duration&limit=120&project={}",
        org(MINE),
        org(7)
    );

    let (status, _) = served(Arc::clone(&asked), looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::OK);
    let filter = asked.0.lock().expect("the lock").clone().expect("a filter");
    assert_eq!(filter.window.from_unix_nanos(), 1_000_000_000);
    assert_eq!(filter.window.to_unix_nanos(), 2_000_000_000);
    assert_eq!(filter.service.as_deref(), Some("payments"));
    assert_eq!(filter.name.as_deref(), Some("psp.charge"));
    assert!(filter.only_errors);
    assert_eq!(filter.at_least_nanos, Some(1_500_000));
    assert_eq!(filter.order, TraceOrder::Duration);
    assert_eq!(filter.limit(), 120);
    assert_eq!(filter.project, Some(ProjectId::new(Uuid::from_u128(7))));
}

#[tokio::test]
async fn a_name_that_was_typed_and_cleared_is_not_a_filter() {
    let asked = Arc::new(Remembering::default());
    let uri = format!(
        "/api/orgs/{}/traces?from=1000&to=2000&name=%20%20",
        org(MINE)
    );

    let (status, _) = served(Arc::clone(&asked), looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        asked
            .0
            .lock()
            .expect("the lock")
            .clone()
            .expect("a filter")
            .name,
        None
    );
}

#[tokio::test]
async fn a_window_on_its_own_filters_nothing_and_orders_by_when_it_started() {
    let asked = Arc::new(Remembering::default());
    let uri = format!("/api/orgs/{}/traces?from=1000&to=2000", org(MINE));

    let (status, _) = served(Arc::clone(&asked), looking(&uri, Some(&session()), true)).await;

    assert_eq!(status, StatusCode::OK);
    let filter = asked.0.lock().expect("the lock").clone().expect("a filter");
    assert_eq!(filter.service, None);
    assert_eq!(filter.name, None);
    assert!(!filter.only_errors);
    assert_eq!(filter.at_least_nanos, None);
    assert_eq!(filter.order, TraceOrder::Started);
    assert_eq!(filter.project, None);
}
