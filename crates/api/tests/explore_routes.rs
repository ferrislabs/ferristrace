use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use ferristrace_api::{AppState, explore_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, AttributeFilter, AttributeKey, DurationRange, Expr, FacetFilter, FacetValues,
    Facets, Field, Membership, Memberships, NoPhotos, OrgRole, Placement, PlacementName,
    PlacementReader, Placements, ProjectId, SeenAttribute, SeenPod, SeenValue, SeenWorkload,
    ServiceFilter, ServicePoint, ServiceSeries, ServiceSummary, Session, SessionError,
    SessionStore, SessionToken, Span, SpanCursor, SpanEvent, SpanId, SpanKind, SpanPage, SpanPages,
    SpanReader, SpanStatus, StoreError, TenantId, Test, TraceFilter, TraceId, TraceOrder,
    TraceSummary, UnixSeconds, WhoamiError, Window, Workload, WorkloadKind,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft.person.0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const TRACE: &str = "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a";

struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
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

fn a_summary(name: &str) -> ServiceSummary {
    ServiceSummary {
        name: name.into(),
        requests: 10,
        errors: 1,
        p50_nanos: 96_000_000,
        p95_nanos: 1_280_000_000,
        p99_nanos: 2_900_000_000,
        last_seen_unix_nanos: 1_700_000_000_123_456_789,
    }
}

impl ferristrace_domain::CulpritReader for TwoSpans {
    async fn culprits(
        &self,
        _org: ferristrace_domain::TenantId,
        _project: Option<ferristrace_domain::ProjectId>,
        _signal: &ferristrace_domain::Signal,
        _window: ferristrace_domain::Window,
    ) -> Result<ferristrace_domain::Culprits, ferristrace_domain::StoreError> {
        Ok(ferristrace_domain::Culprits::default())
    }
}

impl SpanReader for TwoSpans {
    async fn services(
        &self,
        _tenant: TenantId,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(vec![a_summary("checkout")])
    }

    async fn operations(
        &self,
        _tenant: TenantId,
        service: &str,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(vec![a_summary(&format!("{service} POST /pay"))])
    }

    async fn series(
        &self,
        _tenant: TenantId,
        _service: Option<&str>,
        filter: &ServiceFilter,
    ) -> Result<ServiceSeries, StoreError> {
        let start = filter.window.from_unix_nanos();
        Ok(ServiceSeries {
            step_nanos: filter.step_nanos(),
            requests: 7,
            errors: 2,
            p50_nanos: None,
            p95_nanos: Some(3_000_000),
            p99_nanos: None,
            points: vec![ServicePoint {
                start_unix_nanos: start,
                requests: 7,
                errors: 2,
                p95_nanos: Some(3_000_000),
            }],
        })
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
            status_code: Some(500),
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

impl PlacementReader for NoSpans {
    async fn placements(
        &self,
        _tenant: TenantId,
        _window: Window,
        _project: Option<ProjectId>,
    ) -> Result<Placements, StoreError> {
        Ok(Placements::default())
    }
}

impl PlacementReader for TwoSpans {
    async fn placements(
        &self,
        _tenant: TenantId,
        _window: Window,
        _project: Option<ProjectId>,
    ) -> Result<Placements, StoreError> {
        Ok(Placements::default())
    }
}

impl ferristrace_domain::CulpritReader for NoSpans {
    async fn culprits(
        &self,
        _org: ferristrace_domain::TenantId,
        _project: Option<ferristrace_domain::ProjectId>,
        _signal: &ferristrace_domain::Signal,
        _window: ferristrace_domain::Window,
    ) -> Result<ferristrace_domain::Culprits, ferristrace_domain::StoreError> {
        Ok(ferristrace_domain::Culprits::default())
    }
}

impl SpanReader for NoSpans {
    async fn services(
        &self,
        _tenant: TenantId,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn operations(
        &self,
        _tenant: TenantId,
        _service: &str,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn series(
        &self,
        _tenant: TenantId,
        _service: Option<&str>,
        _filter: &ServiceFilter,
    ) -> Result<ServiceSeries, StoreError> {
        Ok(ServiceSeries::default())
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

fn looking(uri: &str, cookie: Option<&str>) -> Request<Body> {
    let mut request = Request::builder().uri(uri);
    if let Some(cookie) = cookie {
        request = request.header(AUTHORIZATION, cookie);
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

impl<
    R: SpanReader
        + SpanPages
        + PlacementReader
        + ferristrace_domain::CulpritReader
        + Send
        + Sync
        + 'static,
> Stack for TestStack<R>
{
    type Repos = Fakes;
    type Clock = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = R;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Users = Bearers;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

fn state<
    R: SpanReader
        + SpanPages
        + PlacementReader
        + ferristrace_domain::CulpritReader
        + Send
        + Sync
        + 'static,
>(
    reader: R,
) -> AppState<TestStack<R>> {
    let adapters = Adapters {
        clock: Inert::default(),
        mailer: Inert::default(),
        webhooks: Inert::default(),
        photos: NoPhotos,
        meter: Inert::default(),
        span_store: Some(reader),
        issue_store: None,
        log_store: None,
        metric_store: None,
        users: Some(Bearers),
    };
    AppState::new(Arc::new(Ferristrace::new(Fakes, adapters)))
}

async fn served<R>(reader: R, request: Request<Body>) -> (StatusCode, String)
where
    R: SpanReader
        + SpanPages
        + PlacementReader
        + ferristrace_domain::CulpritReader
        + Send
        + Sync
        + 'static,
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
    format!("Bearer {TOKEN}")
}

#[tokio::test]
async fn a_member_lists_the_traces_of_its_own_organisation() {
    let uri = format!(
        "/api/orgs/{}/traces?from=1700000000000&to=1700000100000",
        org(MINE)
    );

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::OK);
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    let listed = &found["traces"][0];
    assert_eq!(listed["trace_id"], TRACE);
    assert_eq!(listed["root_service"], "checkout");
    assert_eq!(listed["status_code"], 500);
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

    let (status, _) = served(TwoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
}

#[tokio::test]
async fn a_window_that_ends_before_it_starts_is_refused() {
    let uri = format!("/api/orgs/{}/traces?from=20&to=10", org(MINE));

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("must end after it starts"), "{body}");
}

#[tokio::test]
async fn a_window_nobody_gave_is_refused() {
    let uri = format!("/api/orgs/{}/traces", org(MINE));

    let (status, _) = served(TwoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn one_trace_is_laid_out_for_a_waterfall() {
    let uri = format!("/api/orgs/{}/traces/{TRACE}", org(MINE));

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()))).await;

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

        let (status, _) = served(TwoSpans, looking(&uri, Some(&session()))).await;

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

    let (status, _) = served(NoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_empty_list_is_an_answer_rather_than_an_error() {
    let uri = format!("/api/orgs/{}/traces?from=1&to=2", org(MINE));

    let (status, body) = served(NoSpans, looking(&uri, Some(&session()))).await;

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

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()))).await;

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
    assert_eq!(service["error_rate"], 0.1);
    assert_eq!(service["p50_us"], 96_000);
    assert_eq!(service["p99_us"], 2_900_000);
    assert_eq!(service["last_seen_ms"], 1_700_000_000_123u64);
}

#[tokio::test]
async fn the_services_of_an_organisation_this_session_does_not_hold_are_not_found() {
    let uri = format!("/api/orgs/{}/services?from=1&to=2", org(THEIRS));

    let (status, _) = served(TwoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_services_window_that_ends_before_it_starts_is_refused() {
    let uri = format!("/api/orgs/{}/services?from=20&to=10", org(MINE));

    let (status, _) = served(TwoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_exception_on_a_span_reaches_the_waterfall_with_its_place_in_it() {
    let uri = format!("/api/orgs/{}/traces/{TRACE}", org(MINE));

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()))).await;

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

type ServiceAsked = (Option<String>, ServiceFilter);

#[derive(Debug, Default)]
struct Remembering(
    Mutex<Option<TraceFilter>>,
    Mutex<Vec<ServiceAsked>>,
    Mutex<Vec<(Window, Option<ProjectId>)>>,
    Mutex<Option<(Option<String>, AttributeFilter)>>,
    Mutex<Option<FacetFilter>>,
);

impl PlacementReader for Remembering {
    async fn placements(
        &self,
        _tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Placements, StoreError> {
        self.2.lock().expect("the lock").push((window, project));
        Ok(Placements {
            clusters: vec!["prod".into()],
            namespaces: vec!["shop".into()],
            workloads: vec![SeenWorkload {
                kind: WorkloadKind::StatefulSet,
                name: "postgres".into(),
                namespace: "shop".into(),
            }],
            pods: vec![SeenPod {
                name: "postgres-0".into(),
                namespace: "shop".into(),
                node: "node-a".into(),
            }],
            nodes: vec!["node-a".into()],
        })
    }
}

impl Remembering {
    fn services_asked(&self) -> Vec<ServiceAsked> {
        self.1.lock().expect("the lock").clone()
    }

    fn note(&self, service: Option<&str>, filter: &ServiceFilter) {
        self.1
            .lock()
            .expect("the lock")
            .push((service.map(str::to_owned), filter.clone()));
    }
}

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

impl ferristrace_domain::CulpritReader for Remembering {
    async fn culprits(
        &self,
        _org: ferristrace_domain::TenantId,
        _project: Option<ferristrace_domain::ProjectId>,
        _signal: &ferristrace_domain::Signal,
        _window: ferristrace_domain::Window,
    ) -> Result<ferristrace_domain::Culprits, ferristrace_domain::StoreError> {
        Ok(ferristrace_domain::Culprits::default())
    }
}

impl SpanReader for Remembering {
    async fn services(
        &self,
        _tenant: TenantId,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        self.note(None, filter);
        Ok(Vec::new())
    }

    async fn operations(
        &self,
        _tenant: TenantId,
        service: &str,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        self.note(Some(service), filter);
        Ok(Vec::new())
    }

    async fn series(
        &self,
        _tenant: TenantId,
        service: Option<&str>,
        filter: &ServiceFilter,
    ) -> Result<ServiceSeries, StoreError> {
        self.note(service, filter);
        Ok(ServiceSeries::default())
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

    async fn facets(&self, _tenant: TenantId, filter: &FacetFilter) -> Result<Facets, StoreError> {
        *self.4.lock().expect("the lock") = Some(filter.clone());
        Ok(Facets {
            fields: filter
                .fields()
                .iter()
                .map(|field| FacetValues {
                    field: field.clone(),
                    values: vec![SeenValue {
                        value: "checkout".into(),
                        spans: 12,
                    }],
                })
                .collect(),
            duration: filter.duration.then_some(DurationRange {
                min_nanos: 1_500,
                max_nanos: 2_000_001,
            }),
        })
    }

    async fn attributes(
        &self,
        _tenant: TenantId,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenAttribute>, StoreError> {
        *self.3.lock().expect("the lock") = Some((None, filter.clone()));
        Ok(vec![SeenAttribute {
            key: "rpc.method".into(),
            spans: 7,
            numeric: false,
        }])
    }

    async fn attribute_values(
        &self,
        _tenant: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenValue>, StoreError> {
        *self.3.lock().expect("the lock") = Some((Some(key.as_str().to_owned()), filter.clone()));
        Ok(vec![SeenValue {
            value: "Pay".into(),
            spans: 3,
        }])
    }
}

impl Remembering {
    fn facets_asked(&self) -> Option<FacetFilter> {
        self.4.lock().expect("the lock").clone()
    }
}

fn searching(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(AUTHORIZATION, session())
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .expect("a request")
}

#[tokio::test]
async fn a_posted_query_reaches_the_store_as_a_tree() {
    let asked = Arc::new(Remembering::default());
    let uri = format!("/api/orgs/{}/traces/search", org(MINE));
    let body = r#"{"from":1000,"to":2000,"errors":true,
        "query":{"all":[{"field":"service","op":"eq","value":"checkout"},
                        {"any":[{"attr":"rpc.method","op":"eq","value":"Pay"},
                                {"attr":"retries","op":"gt","value":3}]}]}}"#;

    let (status, _) = served(Arc::clone(&asked), searching(&uri, body)).await;

    assert_eq!(status, StatusCode::OK);
    let filter = asked.0.lock().expect("the lock").clone().expect("a filter");
    assert!(filter.only_errors);
    let Some(Expr::All(parts)) = filter.query.filter() else {
        panic!("an AND at the top");
    };
    assert_eq!(parts.len(), 2);
    let Expr::Is(first) = &parts[0] else {
        panic!("a clause first");
    };
    assert_eq!(first.field(), &Field::Service);
    assert!(matches!(first.test(), Test::Eq(_)));
    assert!(matches!(&parts[1], Expr::Any(any) if any.len() == 2));
}

#[tokio::test]
async fn a_search_without_a_query_asks_for_everything() {
    let asked = Arc::new(Remembering::default());
    let uri = format!("/api/orgs/{}/traces/search", org(MINE));

    let (status, _) = served(
        Arc::clone(&asked),
        searching(&uri, r#"{"from":1000,"to":2000}"#),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let filter = asked.0.lock().expect("the lock").clone().expect("a filter");
    assert!(filter.query.is_anything());
}

#[tokio::test]
async fn a_query_the_domain_refuses_never_reaches_the_store() {
    let uri = format!("/api/orgs/{}/traces/search", org(MINE));
    let nine_deep = (0..8).fold(
        r#"{"attr":"a","op":"eq","value":"x"}"#.to_owned(),
        |inner, _| format!(r#"{{"not":{inner}}}"#),
    );
    let thirty_three = format!(
        r#"{{"all":[{}]}}"#,
        vec![r#"{"attr":"a","op":"exists"}"#; 33].join(",")
    );
    let refused = [
        format!(r#"{{"from":1000,"to":2000,"query":{nine_deep}}}"#),
        format!(r#"{{"from":1000,"to":2000,"query":{thirty_three}}}"#),
        r#"{"from":1000,"to":2000,"query":{"field":"service","op":"gt","value":1}}"#.to_owned(),
        r#"{"from":1000,"to":2000,"query":{"all":[]}}"#.to_owned(),
        r#"{"from":1000,"to":2000,"query":{"attr":"","op":"exists"}}"#.to_owned(),
    ];

    for body in refused {
        let asked = Arc::new(Remembering::default());

        let (status, _) = served(Arc::clone(&asked), searching(&uri, &body)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(asked.0.lock().expect("the lock").is_none(), "{body}");
    }
}

#[tokio::test]
async fn the_catalogue_lists_keys_and_values_for_the_window() {
    let asked = Arc::new(Remembering::default());
    let keys = format!(
        "/api/orgs/{}/attributes?from=1000&to=2000&prefix=rpc&project={}",
        org(MINE),
        org(7)
    );
    let values = format!(
        "/api/orgs/{}/attributes/rpc.method/values?from=1000&to=2000&prefix=P",
        org(MINE)
    );

    let (status, body) = served(Arc::clone(&asked), looking(&keys, Some(&session()))).await;
    let (_, narrowed) = asked.3.lock().expect("the lock").clone().expect("asked");
    let (values_status, values_body) =
        served(Arc::clone(&asked), looking(&values, Some(&session()))).await;
    let (key, by_prefix) = asked.3.lock().expect("the lock").clone().expect("asked");

    assert_eq!(status, StatusCode::OK);
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(found["attributes"][0]["key"], "rpc.method");
    assert_eq!(found["attributes"][0]["spans"], 7);
    assert_eq!(found["attributes"][0]["numeric"], false);
    assert_eq!(narrowed.prefix.as_deref(), Some("rpc"));
    assert_eq!(narrowed.project, Some(ProjectId::new(Uuid::from_u128(7))));
    assert_eq!(values_status, StatusCode::OK);
    let listed: serde_json::Value = serde_json::from_str(&values_body).expect("json");
    assert_eq!(listed["values"][0]["value"], "Pay");
    assert_eq!(key.as_deref(), Some("rpc.method"));
    assert_eq!(by_prefix.prefix.as_deref(), Some("P"));
}

#[tokio::test]
async fn an_unsigned_catalogue_request_is_refused() {
    let uri = format!("/api/orgs/{}/attributes?from=1000&to=2000", org(MINE));

    let (status, _) = served(Arc::new(Remembering::default()), looking(&uri, None)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
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

    let (status, _) = served(Arc::clone(&asked), looking(&uri, Some(&session()))).await;

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

    let (status, _) = served(Arc::clone(&asked), looking(&uri, Some(&session()))).await;

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

    let (status, _) = served(Arc::clone(&asked), looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::OK);
    let filter = asked.0.lock().expect("the lock").clone().expect("a filter");
    assert_eq!(filter.service, None);
    assert_eq!(filter.name, None);
    assert!(!filter.only_errors);
    assert_eq!(filter.at_least_nanos, None);
    assert_eq!(filter.order, TraceOrder::Started);
    assert_eq!(filter.project, None);
}

#[tokio::test]
async fn the_operations_of_a_service_are_listed_with_their_measures_in_nanoseconds() {
    let uri = format!(
        "/api/orgs/{}/services/checkout/operations?from=1700000000000&to=1700000060000",
        org(MINE)
    );

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    let operation = &found["operations"][0];
    assert_eq!(operation["name"], "checkout POST /pay");
    assert_eq!(operation["requests"], 10);
    assert_eq!(operation["errors"], 1);
    assert_eq!(operation["p50_ns"], 96_000_000);
    assert_eq!(operation["p95_ns"], 1_280_000_000u64);
    assert_eq!(operation["p99_ns"], 2_900_000_000u64);
    assert_eq!(operation["last_seen_ns"], 1_700_000_000_123_456_789u64);
}

#[tokio::test]
async fn a_service_series_has_sixty_steps_of_requests_errors_and_p95() {
    let uri = format!(
        "/api/orgs/{}/services/checkout/series?from=1700000000000&to=1700003600000",
        org(MINE)
    );

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(found["step_ns"], 60_000_000_000u64);
    let points = found["points"].as_array().expect("points");
    assert_eq!(points.len(), 60);
    assert_eq!(
        points[0],
        serde_json::json!([1_700_000_000_000_000_000u64, 7, 2, 3_000_000])
    );
    assert_eq!(
        points[1],
        serde_json::json!([1_700_000_060_000_000_000u64, 0, 0, null])
    );
    assert_eq!(found["requests"], 7);
    assert_eq!(found["errors"], 2);
    assert_eq!(found["p95_ns"], 3_000_000);
}

#[tokio::test]
async fn the_organisation_series_is_the_same_shape_over_every_service() {
    let uri = format!(
        "/api/orgs/{}/series?from=1700000000000&to=1700003600000",
        org(MINE)
    );

    let (status, body) = served(TwoSpans, looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(found["points"].as_array().map(Vec::len), Some(60));
}

#[tokio::test]
async fn the_service_routes_refuse_strangers_bad_windows_and_missing_sessions() {
    for route in [
        "services/checkout/operations",
        "services/checkout/series",
        "series",
    ] {
        let theirs = format!("/api/orgs/{}/{route}?from=1&to=2", org(THEIRS));
        let backwards = format!("/api/orgs/{}/{route}?from=20&to=10", org(MINE));
        let mine = format!("/api/orgs/{}/{route}?from=1&to=2", org(MINE));

        let (stranger, _) = served(TwoSpans, looking(&theirs, Some(&session()))).await;
        let (window, _) = served(TwoSpans, looking(&backwards, Some(&session()))).await;
        let (anonymous, _) = served(TwoSpans, looking(&mine, None)).await;

        assert_eq!(stranger, StatusCode::NOT_FOUND, "{route}");
        assert_eq!(window, StatusCode::BAD_REQUEST, "{route}");
        assert_eq!(anonymous, StatusCode::UNAUTHORIZED, "{route}");
    }
}

#[tokio::test]
async fn the_service_routes_pass_the_project_environment_and_service_to_the_store() {
    let asked = Arc::new(Remembering::default());
    let project = org(7);
    for route in [
        "services",
        "services/check%20out/operations",
        "services/check%20out/series",
        "series",
    ] {
        let uri = format!(
            "/api/orgs/{}/{route}?from=1000&to=2000&project={project}&environment=production",
            org(MINE)
        );

        let (status, body) = served(Arc::clone(&asked), looking(&uri, Some(&session()))).await;

        assert_eq!(status, StatusCode::OK, "{route}: {body}");
    }

    let services: Vec<Option<String>> = asked
        .services_asked()
        .into_iter()
        .map(|(service, _)| service)
        .collect();
    assert_eq!(
        services,
        vec![
            None,
            Some("check out".to_owned()),
            Some("check out".to_owned()),
            None
        ]
    );
    for (_, filter) in asked.services_asked() {
        assert_eq!(filter.window.from_unix_nanos(), 1_000_000_000);
        assert_eq!(filter.project, Some(ProjectId::new(Uuid::from_u128(7))));
        assert_eq!(filter.environment.as_deref(), Some("production"));
    }
}

#[tokio::test]
async fn an_empty_environment_is_no_filter() {
    let asked = Arc::new(Remembering::default());
    let uri = format!(
        "/api/orgs/{}/services?from=1000&to=2000&environment=",
        org(MINE)
    );

    let (status, _) = served(Arc::clone(&asked), looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(asked.services_asked()[0].1.environment, None);
}

fn placed() -> Placement {
    Placement {
        cluster: Some(PlacementName::try_from("prod").expect("a cluster")),
        namespace: Some(PlacementName::try_from("shop").expect("a namespace")),
        workload: Some(Workload::try_from("statefulset:postgres").expect("a workload")),
        pod: Some(PlacementName::try_from("postgres-0").expect("a pod")),
        node: Some(PlacementName::try_from("node-a").expect("a node")),
    }
}

const PLACED: &str =
    "cluster=prod&namespace=shop&workload=statefulset:postgres&pod=postgres-0&node=node-a";

#[tokio::test]
async fn a_placement_reaches_the_store_from_every_explore_route() {
    let asked = Arc::new(Remembering::default());
    let org = org(MINE);
    for route in [
        format!("/api/orgs/{org}/traces?from=1000&to=2000&{PLACED}"),
        format!("/api/orgs/{org}/services?from=1000&to=2000&{PLACED}"),
        format!("/api/orgs/{org}/services/checkout/operations?from=1000&to=2000&{PLACED}"),
        format!("/api/orgs/{org}/services/checkout/series?from=1000&to=2000&{PLACED}"),
        format!("/api/orgs/{org}/series?from=1000&to=2000&{PLACED}"),
    ] {
        let (status, body) = served(Arc::clone(&asked), looking(&route, Some(&session()))).await;

        assert_eq!(status, StatusCode::OK, "{route}: {body}");
    }

    let traced = asked.0.lock().expect("the lock").clone().expect("a filter");
    assert_eq!(traced.placement, placed());
    let services = asked.services_asked();
    assert_eq!(services.len(), 4);
    for (_, filter) in services {
        assert_eq!(filter.placement, placed());
    }
}

#[tokio::test]
async fn a_placement_left_blank_filters_nothing() {
    let asked = Arc::new(Remembering::default());
    let uri = format!(
        "/api/orgs/{}/services?from=1000&to=2000&pod=&node=%20",
        org(MINE)
    );

    let (status, _) = served(Arc::clone(&asked), looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::OK);
    assert!(asked.services_asked()[0].1.placement.is_empty());
}

#[tokio::test]
async fn a_placement_that_is_no_kubernetes_name_is_refused_before_the_store_is_asked() {
    let asked = Arc::new(Remembering::default());
    let org = org(MINE);
    for (route, field) in [
        (
            format!("/api/orgs/{org}/traces?from=1000&to=2000&pod=a%20b"),
            "pod",
        ),
        (
            format!("/api/orgs/{org}/services?from=1000&to=2000&workload=replicaset:api"),
            "workload",
        ),
        (
            format!("/api/orgs/{org}/series?from=1000&to=2000&namespace=-shop"),
            "namespace",
        ),
        (
            format!(
                "/api/orgs/{org}/services/checkout/series?from=1000&to=2000&node={}",
                "n".repeat(254)
            ),
            "node",
        ),
        (
            format!("/api/orgs/{org}/placements?from=1000&to=2000&project=nope"),
            "project",
        ),
    ] {
        let (status, body) = served(Arc::clone(&asked), looking(&route, Some(&session()))).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{route}: {body}");
        if field != "project" {
            assert!(body.contains(field), "{route}: {body}");
        }
    }
    assert!(asked.0.lock().expect("the lock").is_none());
    assert!(asked.services_asked().is_empty());
    assert!(asked.2.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn the_placements_seen_in_a_window_feed_the_pickers() {
    let asked = Arc::new(Remembering::default());
    let uri = format!(
        "/api/orgs/{}/placements?from=1000&to=2000&project={}",
        org(MINE),
        org(7)
    );

    let (status, body) = served(Arc::clone(&asked), looking(&uri, Some(&session()))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let seen: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        seen,
        serde_json::json!({
            "clusters": ["prod"],
            "namespaces": ["shop"],
            "workloads": [{"kind": "statefulset", "name": "postgres", "namespace": "shop"}],
            "pods": [{"name": "postgres-0", "namespace": "shop", "node": "node-a"}],
            "nodes": ["node-a"],
        })
    );
    let (window, project) = asked.2.lock().expect("the lock")[0];
    assert_eq!(window.from_unix_nanos(), 1_000_000_000);
    assert_eq!(window.to_unix_nanos(), 2_000_000_000);
    assert_eq!(project, Some(ProjectId::new(Uuid::from_u128(7))));
}

#[tokio::test]
async fn the_placements_of_a_stranger_or_a_backwards_window_are_refused() {
    let theirs = format!("/api/orgs/{}/placements?from=1000&to=2000", org(THEIRS));
    let backwards = format!("/api/orgs/{}/placements?from=2000&to=1000", org(MINE));

    let (stranger, _) = served(NoSpans, looking(&theirs, Some(&session()))).await;
    let (window, _) = served(NoSpans, looking(&backwards, Some(&session()))).await;
    let (anonymous, _) = served(NoSpans, looking(&theirs, None)).await;

    assert_eq!(stranger, StatusCode::NOT_FOUND);
    assert_eq!(window, StatusCode::BAD_REQUEST);
    assert_eq!(anonymous, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_request_without_a_session_is_refused() {
    let uri = format!("/api/orgs/{}/traces?from=1&to=2", org(MINE));

    let (no_bearer, _) = served(TwoSpans, looking(&uri, None)).await;
    let (unknown, _) = served(TwoSpans, looking(&uri, Some("Bearer ft.person.nope"))).await;
    let (not_a_person, _) = served(TwoSpans, looking(&uri, Some("Bearer ft.agent.nope"))).await;

    assert_eq!(no_bearer, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown, StatusCode::UNAUTHORIZED);
    assert_eq!(not_a_person, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn facets_are_counted_under_the_query_and_come_back_named_as_they_were_asked() {
    let asked = Arc::new(Remembering::default());
    let uri = format!("/api/orgs/{}/traces/facets", org(MINE));
    let body = format!(
        r#"{{"from":1000,"to":2000,"project":"{}","duration":true,
            "query":{{"field":"status","op":"eq","value":"error"}},
            "facets":[{{"field":"service"}},{{"attr":"http.method"}}]}}"#,
        org(7)
    );

    let (status, found) = served(Arc::clone(&asked), searching(&uri, &body)).await;

    assert_eq!(status, StatusCode::OK);
    let counted = serde_json::from_str::<serde_json::Value>(&found).expect("json");
    assert_eq!(counted["facets"][0]["field"], "service");
    assert_eq!(counted["facets"][0]["values"][0]["value"], "checkout");
    assert_eq!(counted["facets"][0]["values"][0]["spans"], 12);
    assert_eq!(counted["facets"][1]["attr"], "http.method");
    assert_eq!(counted["duration"]["min_us"], 1);
    assert_eq!(counted["duration"]["max_us"], 2_001);
    let filter = asked.facets_asked().expect("asked");
    assert!(filter.duration);
    assert_eq!(filter.fields().len(), 2);
    assert!(!filter.query.is_anything());
    assert_eq!(filter.project, Some(ProjectId::new(Uuid::from_u128(7))));
}

#[tokio::test]
async fn a_facet_request_the_domain_refuses_is_a_bad_request() {
    let uri = format!("/api/orgs/{}/traces/facets", org(MINE));
    let many = vec![r#"{"attr":"k"}"#; 13].join(",");
    let refused = [
        format!(r#"{{"from":1000,"to":2000,"facets":[{many}]}}"#),
        r#"{"from":1000,"to":2000,"facets":[{"field":"trace_id"}]}"#.to_owned(),
        r#"{"from":1000,"to":2000,"facets":[{"field":"duration"}]}"#.to_owned(),
        r#"{"from":1000,"to":2000,"facets":[{"field":"service","attr":"x"}]}"#.to_owned(),
        r#"{"from":1000,"to":2000,"query":{"all":[]},"facets":[]}"#.to_owned(),
        r#"{"from":2000,"to":1000,"facets":[]}"#.to_owned(),
    ];

    for body in refused {
        let asked = Arc::new(Remembering::default());

        let (status, _) = served(Arc::clone(&asked), searching(&uri, &body)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(asked.facets_asked().is_none(), "{body}");
    }
}

#[tokio::test]
async fn an_unsigned_facet_request_is_refused() {
    let uri = format!("/api/orgs/{}/traces/facets", org(MINE));
    let unsigned = Request::builder()
        .method("POST")
        .uri(&uri)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"from":1000,"to":2000}"#))
        .expect("a request");

    let (status, _) = served(Arc::new(Remembering::default()), unsigned).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
