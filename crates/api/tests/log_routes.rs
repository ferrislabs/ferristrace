use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use ferristrace_api::{AppState, log_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, ApiKey, ApiKeys, ApiSecret, Buckets, IssuedKey, KeyId, KeyName, KeyScopes,
    LogBucket, LogCursor, LogPage, LogQuery, LogReader, LogRecord, Membership, Memberships,
    NoPhotos, OrgRole, ProjectId, ReadGrant, Session, SessionError, SessionStore, SessionToken,
    Severity, SpanId, StoreError, TenantId, TraceId, UnixSeconds, WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft.person.0123456789";
const ORG_KEY: &str = "ft_rd_whole_org";
const PROJECT_KEY: &str = "ft_rd_one_project";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const OURS: u128 = 30;
const OTHER: u128 = 40;
const TRACE: &str = "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a";
const SPAN: &str = "0b0b0b0b0b0b0b0b";
const FROM_MS: u64 = 1_700_000_000_000;
const TO_MS: u64 = 1_700_000_060_000;

struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
            Ok(Session {
                account: AccountId::new(Uuid::from_u128(9)),
                email: ferristrace_domain::Email::try_from("person@example.com")
                    .expect("an address"),
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
            org: org_id(MINE),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

struct TwoReadKeys;

impl ApiKeys for TwoReadKeys {
    async fn list(&self, _org: TenantId) -> Result<Vec<ApiKey>, StoreError> {
        Ok(Vec::new())
    }

    async fn issue(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _name: &KeyName,
        _scopes: &KeyScopes,
        _by: AccountId,
    ) -> Result<IssuedKey, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn revoke(&self, _org: TenantId, _id: KeyId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn grant_for(&self, secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(match secret.exposed() {
            ORG_KEY => Some(ReadGrant::whole(org_id(MINE))),
            PROJECT_KEY => Some(ReadGrant {
                org: org_id(MINE),
                project: Some(project_id(OURS)),
            }),
            _ => None,
        })
    }
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
    type Repo = TwoReadKeys;

    fn build(&self, (): &()) -> Self::Repo {
        TwoReadKeys
    }
}

#[derive(Debug, Default)]
struct Shelf {
    asked: Mutex<Vec<(TenantId, LogQuery, Option<Buckets>)>>,
}

impl Shelf {
    fn asked(&self) -> Vec<(TenantId, LogQuery, Option<Buckets>)> {
        self.asked.lock().expect("the lock").clone()
    }
}

fn a_record() -> LogRecord {
    LogRecord {
        time_unix_nanos: 1_700_000_001_123_456_789,
        observed_unix_nanos: 1_700_000_001_123_456_999,
        severity: Severity::Error,
        severity_number: 17,
        service_name: "payments".into(),
        body: "card declined".into(),
        attributes: BTreeMap::from([("order.id".into(), "o-42".into())]),
        resource: BTreeMap::from([("host.name".into(), "web-1".into())]),
        trace_id: Some(trace_id()),
        span_id: Some(
            SpanId::try_from(hex::decode(SPAN).expect("hex").as_slice()).expect("eight bytes"),
        ),
    }
}

struct Store(Arc<Shelf>);

impl ferristrace_domain::CulpritReader for Store {
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

impl LogReader for Store {
    async fn search(&self, org: TenantId, query: &LogQuery) -> Result<LogPage, StoreError> {
        self.0
            .asked
            .lock()
            .expect("the lock")
            .push((org, query.clone(), None));
        let mut quiet = a_record();
        quiet.severity = Severity::Info;
        quiet.trace_id = None;
        quiet.span_id = None;
        Ok(LogPage {
            records: vec![a_record(), quiet],
            next: Some(LogCursor {
                time_unix_nanos: 1_700_000_000_500_000_000,
                tiebreak: 77,
            }),
        })
    }

    async fn histogram(
        &self,
        org: TenantId,
        query: &LogQuery,
        buckets: Buckets,
    ) -> Result<Vec<LogBucket>, StoreError> {
        self.0
            .asked
            .lock()
            .expect("the lock")
            .push((org, query.clone(), Some(buckets)));
        let mut laid = buckets.laid_over(query.window);
        laid[0].add(Severity::Error, 2);
        laid[0].add(Severity::Info, 5);
        Ok(laid)
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
    type LogStore = Store;
    type MetricStore = Inert;
    type Meter = Inert;
    type Users = Bearers;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

fn state(shelf: Option<Arc<Shelf>>) -> AppState<TestStack> {
    let adapters = Adapters {
        clock: Inert::default(),
        mailer: Inert::default(),
        webhooks: Inert::default(),
        photos: NoPhotos,
        meter: Inert::default(),
        span_store: None,
        issue_store: None,
        log_store: shelf.map(Store),
        metric_store: None,
        users: Some(Bearers),
    };
    AppState::new(Arc::new(Ferristrace::new(Fakes, adapters)))
}

enum Who {
    Session,
    Key(&'static str),
    Nobody,
}

fn looking(uri: &str, who: &Who) -> Request<Body> {
    let request = Request::builder().uri(uri);
    let request = match who {
        Who::Session => request.header(AUTHORIZATION, format!("Bearer {TOKEN}")),
        Who::Key(key) => request.header(AUTHORIZATION, format!("Bearer {key}")),
        Who::Nobody => request,
    };
    request.body(Body::empty()).expect("a request")
}

async fn served(shelf: Option<Arc<Shelf>>, request: Request<Body>) -> (StatusCode, String) {
    let response = log_router(state(shelf))
        .oneshot(request)
        .await
        .expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("text"))
}

fn org_id(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn project_id(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn trace_id() -> TraceId {
    TraceId::try_from(hex::decode(TRACE).expect("hex").as_slice()).expect("sixteen bytes")
}

fn logs_of(seed: u128, rest: &str) -> String {
    format!(
        "/api/orgs/{}/logs?from={FROM_MS}&to={TO_MS}{rest}",
        Uuid::from_u128(seed)
    )
}

fn histogram_of(seed: u128, rest: &str) -> String {
    format!(
        "/api/orgs/{}/logs/histogram?from={FROM_MS}&to={TO_MS}{rest}",
        Uuid::from_u128(seed)
    )
}

#[tokio::test]
async fn a_member_reads_records_in_the_agreed_shape() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(&logs_of(MINE, ""), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        found["records"][0],
        serde_json::json!({
            "time_ns": 1_700_000_001_123_456_789u64,
            "observed_ns": 1_700_000_001_123_456_999u64,
            "severity": "error",
            "service": "payments",
            "body": "card declined",
            "trace_id": TRACE,
            "span_id": SPAN,
            "attributes": {"order.id": "o-42"},
            "resource": {"host.name": "web-1"},
        })
    );
    assert_eq!(found["records"][1]["severity"], "info");
    assert_eq!(found["records"][1]["trace_id"], serde_json::Value::Null);
    assert_eq!(found["records"][1]["span_id"], serde_json::Value::Null);
    assert_eq!(found["next"], "1700000000500000000.77");
    let (org, query, _) = &shelf.asked()[0];
    assert_eq!(*org, org_id(MINE));
    assert_eq!(query.window.from_unix_nanos(), FROM_MS * 1_000_000);
    assert_eq!(query.window.to_unix_nanos(), TO_MS * 1_000_000);
    assert_eq!(query.limit(), 100);
}

#[tokio::test]
async fn every_filter_reaches_the_log_store() {
    let shelf = Arc::new(Shelf::default());
    let rest = format!(
        "&service=checkout&service=payments&severity=error&severity=fatal&q=declined\
         &attr=http.route%3D%2Fpay&attr=region=eu=west&trace={TRACE}&project={}&limit=20\
         &before=1700000000500000000.77",
        Uuid::from_u128(OURS)
    );

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(&logs_of(MINE, &rest), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, query, _) = &shelf.asked()[0];
    assert_eq!(
        query.services,
        vec![Box::from("checkout"), Box::from("payments")]
    );
    assert_eq!(query.severities, vec![Severity::Error, Severity::Fatal]);
    assert_eq!(query.text.as_deref(), Some("declined"));
    assert_eq!(
        query.attributes,
        vec![
            (Box::from("http.route"), Box::from("/pay")),
            (Box::from("region"), Box::from("eu=west")),
        ]
    );
    assert_eq!(query.trace, Some(trace_id()));
    assert_eq!(query.project, Some(project_id(OURS)));
    assert_eq!(query.limit(), 20);
    assert_eq!(
        query.before,
        Some(LogCursor {
            time_unix_nanos: 1_700_000_000_500_000_000,
            tiebreak: 77,
        })
    );
}

#[tokio::test]
async fn the_histogram_answers_counts_per_severity_per_bucket() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(
            &histogram_of(MINE, "&buckets=6&severity=error&limit=0&before=nonsense"),
            &Who::Session,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    let buckets = found["buckets"].as_array().expect("buckets");
    assert_eq!(buckets.len(), 6);
    assert_eq!(
        buckets[0],
        serde_json::json!({
            "start_ns": FROM_MS * 1_000_000,
            "counts": {"trace": 0, "debug": 0, "info": 5, "warn": 0, "error": 2, "fatal": 0},
        })
    );
    assert_eq!(buckets[1]["start_ns"], (FROM_MS + 10_000) * 1_000_000);
    let (_, query, asked) = &shelf.asked()[0];
    assert_eq!(asked.map(Buckets::count), Some(6));
    assert_eq!(query.severities, vec![Severity::Error]);
    assert_eq!(query.before, None, "a histogram is not paged");
}

#[tokio::test]
async fn the_histogram_asks_for_sixty_buckets_by_default() {
    let shelf = Arc::new(Shelf::default());

    let (status, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&histogram_of(MINE, ""), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(shelf.asked()[0].2.map(Buckets::count), Some(60));
}

#[tokio::test]
async fn an_organisation_this_session_does_not_belong_to_is_not_found() {
    let shelf = Arc::new(Shelf::default());

    let (records, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&logs_of(THEIRS, ""), &Who::Session),
    )
    .await;
    let (histogram, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&histogram_of(THEIRS, ""), &Who::Session),
    )
    .await;

    assert_eq!(records, StatusCode::NOT_FOUND);
    assert_eq!(histogram, StatusCode::NOT_FOUND);
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn a_read_key_reads_the_logs_of_its_organisation() {
    let shelf = Arc::new(Shelf::default());

    let (status, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&logs_of(MINE, ""), &Who::Key(ORG_KEY)),
    )
    .await;
    let (stranger, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&logs_of(THEIRS, ""), &Who::Key(ORG_KEY)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(stranger, StatusCode::NOT_FOUND);
    assert_eq!(shelf.asked().len(), 1);
    assert_eq!(shelf.asked()[0].1.project, None);
}

#[tokio::test]
async fn a_project_bound_read_key_reads_only_its_project() {
    let shelf = Arc::new(Shelf::default());
    let other = format!("&project={}", Uuid::from_u128(OTHER));

    let (records, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&logs_of(MINE, ""), &Who::Key(PROJECT_KEY)),
    )
    .await;
    let (histogram, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&histogram_of(MINE, ""), &Who::Key(PROJECT_KEY)),
    )
    .await;
    let (elsewhere, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&logs_of(MINE, &other), &Who::Key(PROJECT_KEY)),
    )
    .await;

    assert_eq!((records, histogram), (StatusCode::OK, StatusCode::OK));
    assert_eq!(elsewhere, StatusCode::NOT_FOUND);
    assert_eq!(
        shelf
            .asked()
            .iter()
            .map(|(_, query, _)| query.project)
            .collect::<Vec<_>>(),
        vec![Some(project_id(OURS)), Some(project_id(OURS))]
    );
}

#[tokio::test]
async fn a_reader_nobody_vouches_for_is_refused() {
    let shelf = Arc::new(Shelf::default());

    let (nobody, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&logs_of(MINE, ""), &Who::Nobody),
    )
    .await;
    let (unknown, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&logs_of(MINE, ""), &Who::Key("ft_in_ingest")),
    )
    .await;

    assert_eq!(nobody, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown, StatusCode::UNAUTHORIZED);
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn malformed_parameters_are_refused_before_the_store_is_asked() {
    let shelf = Arc::new(Shelf::default());
    let mine = Uuid::from_u128(MINE);
    let cases = [
        (format!("/api/orgs/{mine}/logs"), StatusCode::BAD_REQUEST),
        (
            format!("/api/orgs/{mine}/logs?from=20&to=10"),
            StatusCode::BAD_REQUEST,
        ),
        (logs_of(MINE, "&severity=loud"), StatusCode::BAD_REQUEST),
        (logs_of(MINE, "&trace=xyz"), StatusCode::BAD_REQUEST),
        (logs_of(MINE, "&attr=novalue"), StatusCode::BAD_REQUEST),
        (logs_of(MINE, "&pod=a%20b"), StatusCode::BAD_REQUEST),
        (
            logs_of(MINE, "&workload=replicaset:api"),
            StatusCode::BAD_REQUEST,
        ),
        (histogram_of(MINE, "&node=-n"), StatusCode::BAD_REQUEST),
        (logs_of(MINE, "&project=nope"), StatusCode::BAD_REQUEST),
        (logs_of(MINE, "&before=nope"), StatusCode::BAD_REQUEST),
        (logs_of(MINE, "&limit=many"), StatusCode::BAD_REQUEST),
        (logs_of(MINE, "&limit=0"), StatusCode::UNPROCESSABLE_ENTITY),
        (
            logs_of(MINE, "&limit=501"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            histogram_of(MINE, "&buckets=0"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            histogram_of(MINE, "&buckets=201"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ];

    for (uri, expected) in cases {
        let (status, body) = served(Some(Arc::clone(&shelf)), looking(&uri, &Who::Session)).await;
        assert_eq!(status, expected, "{uri}: {body}");
    }
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn without_a_log_store_the_logs_are_unavailable() {
    let (records, _) = served(None, looking(&logs_of(MINE, ""), &Who::Session)).await;
    let (histogram, _) = served(None, looking(&histogram_of(MINE, ""), &Who::Session)).await;

    assert_eq!(records, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(histogram, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn the_log_routes_only_read() {
    let shelf = Arc::new(Shelf::default());
    let request = Request::builder()
        .method("POST")
        .uri(logs_of(MINE, ""))
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .expect("a request");

    let (status, _) = served(Some(Arc::clone(&shelf)), request).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn a_placement_reaches_the_log_store_from_both_routes() {
    let shelf = Arc::new(Shelf::default());
    let placed = "&cluster=prod&namespace=shop&workload=checkout&pod=checkout-1&node=node-a";

    for uri in [logs_of(MINE, placed), histogram_of(MINE, placed)] {
        let (status, body) = served(Some(Arc::clone(&shelf)), looking(&uri, &Who::Session)).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    }

    let asked = shelf.asked();
    assert_eq!(asked.len(), 2);
    for (_, query, _) in asked {
        let placement = &query.placement;
        assert_eq!(
            placement
                .cluster
                .as_ref()
                .map(ferristrace_domain::PlacementName::as_str),
            Some("prod")
        );
        assert_eq!(
            placement
                .namespace
                .as_ref()
                .map(ferristrace_domain::PlacementName::as_str),
            Some("shop")
        );
        assert_eq!(
            placement
                .workload
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("checkout")
        );
        assert_eq!(
            placement
                .pod
                .as_ref()
                .map(ferristrace_domain::PlacementName::as_str),
            Some("checkout-1")
        );
        assert_eq!(
            placement
                .node
                .as_ref()
                .map(ferristrace_domain::PlacementName::as_str),
            Some("node-a")
        );
    }
}

fn posting(path: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/orgs/{}/logs/{path}", Uuid::from_u128(MINE)))
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .expect("a request")
}

#[tokio::test]
async fn a_query_tree_reaches_the_log_store() {
    let shelf = Arc::new(Shelf::default());
    let body = format!(
        r#"{{"from":{FROM_MS},"to":{TO_MS},"limit":20,"query":{{"all":[
            {{"field":"severity","op":"in","value":["error","fatal"]}},
            {{"not":{{"field":"body","op":"contains","value":"health"}}}},
            {{"attr":"http.route","op":"eq","value":"/pay"}}]}}}}"#
    );

    let (status, text) = served(Some(Arc::clone(&shelf)), posting("search", &body)).await;

    assert_eq!(status, StatusCode::OK, "{text}");
    let (_, query, _) = &shelf.asked()[0];
    assert!(!query.query.is_anything());
    assert_eq!(query.limit(), 20);
}

#[tokio::test]
async fn a_span_field_in_a_log_query_is_refused_before_the_store() {
    let shelf = Arc::new(Shelf::default());
    let body = format!(
        r#"{{"from":{FROM_MS},"to":{TO_MS},"query":{{"field":"duration","op":"gt","value":5}}}}"#
    );

    let (status, _) = served(Some(Arc::clone(&shelf)), posting("search", &body)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn the_series_route_counts_per_severity_for_a_query() {
    let shelf = Arc::new(Shelf::default());
    let body = format!(
        r#"{{"from":{FROM_MS},"to":{TO_MS},"query":{{"field":"service","op":"eq","value":"checkout"}}}}"#
    );

    let (status, text) = served(Some(Arc::clone(&shelf)), posting("series", &body)).await;

    assert_eq!(status, StatusCode::OK, "{text}");
    assert!(text.contains("buckets"));
    assert!(!shelf.asked()[0].1.query.is_anything());
}

#[tokio::test]
async fn a_placement_rides_with_a_query_tree() {
    let shelf = Arc::new(Shelf::default());
    let body =
        format!(r#"{{"from":{FROM_MS},"to":{TO_MS},"namespace":"shop","pod":"checkout-1"}}"#);

    let (status, text) = served(Some(Arc::clone(&shelf)), posting("search", &body)).await;

    assert_eq!(status, StatusCode::OK, "{text}");
    let (_, query, _) = &shelf.asked()[0];
    assert_eq!(
        query.placement.namespace.as_ref().map(|name| name.as_str()),
        Some("shop")
    );
    assert_eq!(
        query.placement.pod.as_ref().map(|name| name.as_str()),
        Some("checkout-1")
    );
}
