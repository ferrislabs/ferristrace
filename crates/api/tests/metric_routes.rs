use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use ferristrace_api::{AppState, metric_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Aggregation, ApiKey, ApiKeys, ApiSecret, ClusterFilter, ClusterReader,
    ClusterSnapshot, Exemplar, InfraFilter, InfraReader, IssuedKey, KeyId, KeyName, KeyScopes,
    Membership, Memberships, MetricDescriptor, MetricKind, MetricQuery, MetricReader, NoPhotos,
    OrgRole, ProjectId, ReadGrant, Resource, Series, SeriesSet, Session, SessionError,
    SessionStore, SessionToken, SpanId, StoreError, TenantId, TraceId, UnixSeconds, WhoamiError,
    Window,
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

type Listed = (TenantId, Option<String>, Option<ProjectId>);
type Charted = (TenantId, MetricKind, Aggregation, MetricQuery);

#[derive(Debug, Default)]
struct Shelf {
    listed: Mutex<Vec<Listed>>,
    charted: Mutex<Vec<Charted>>,
}

impl Shelf {
    fn listed(&self) -> Vec<Listed> {
        self.listed.lock().expect("the lock").clone()
    }

    fn charted(&self) -> Vec<Charted> {
        self.charted.lock().expect("the lock").clone()
    }
}

const ROUTE_METRIC: &str = "http.server/requests";

fn described(name: &str, kind: MetricKind, unit: &str) -> MetricDescriptor {
    MetricDescriptor {
        name: name.into(),
        kind,
        unit: unit.into(),
        description: format!("about {name}").into(),
        last_seen_unix_nanos: 1_700_000_001_000_000_000,
    }
}

struct Store(Arc<Shelf>);

impl MetricReader for Store {
    async fn catalogue(
        &self,
        org: TenantId,
        search: Option<&str>,
        project: Option<ProjectId>,
    ) -> Result<Vec<MetricDescriptor>, StoreError> {
        self.0
            .listed
            .lock()
            .expect("the lock")
            .push((org, search.map(str::to_owned), project));
        Ok(vec![
            described(ROUTE_METRIC, MetricKind::Sum, "{request}"),
            described("http.server.duration", MetricKind::Histogram, "ms"),
            described("queue.depth", MetricKind::Gauge, "1"),
        ]
        .into_iter()
        .filter(|metric| search.is_none_or(|text| metric.name.contains(text)))
        .collect())
    }

    async fn series(
        &self,
        org: TenantId,
        kind: MetricKind,
        aggregation: Aggregation,
        query: &MetricQuery,
    ) -> Result<SeriesSet, StoreError> {
        self.0
            .charted
            .lock()
            .expect("the lock")
            .push((org, kind, aggregation, query.clone()));
        let start = query.window.from_unix_nanos();
        Ok(SeriesSet {
            step_nanos: query.step_nanos(),
            series: vec![Series {
                labels: BTreeMap::from([("http.route".into(), "/pay".into())]),
                points: vec![(start, Some(2.5)), (start + query.step_nanos(), None)],
            }],
            exemplars: vec![
                Exemplar {
                    time_unix_nanos: start + 7,
                    value: 12.5,
                    trace_id: Some(trace_id()),
                    span_id: Some(
                        SpanId::try_from(hex::decode(SPAN).expect("hex").as_slice())
                            .expect("eight bytes"),
                    ),
                    attributes: BTreeMap::new(),
                },
                Exemplar {
                    time_unix_nanos: start + 8,
                    value: 1.0,
                    trace_id: None,
                    span_id: None,
                    attributes: BTreeMap::new(),
                },
            ],
        })
    }
}

impl ClusterReader for Store {
    async fn snapshot(
        &self,
        _org: TenantId,
        _window: Window,
        _filter: &ClusterFilter,
    ) -> Result<ClusterSnapshot, StoreError> {
        Ok(ClusterSnapshot::default())
    }
}

impl InfraReader for Store {
    async fn resources(
        &self,
        _org: TenantId,
        _figures: Window,
        _filter: &InfraFilter,
    ) -> Result<Vec<Resource>, StoreError> {
        Ok(Vec::new())
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
    type MetricStore = Store;
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
        log_store: None,
        metric_store: shelf.map(Store),
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
    let response = metric_router(state(shelf))
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

fn catalogue_of(seed: u128, rest: &str) -> String {
    format!("/api/orgs/{}/metrics?x=1{rest}", Uuid::from_u128(seed))
}

fn series_of(seed: u128, encoded_name: &str, rest: &str) -> String {
    format!(
        "/api/orgs/{}/metrics/{encoded_name}/series?from={FROM_MS}&to={TO_MS}{rest}",
        Uuid::from_u128(seed)
    )
}

const ENCODED: &str = "http.server%2Frequests";

#[tokio::test]
async fn a_member_reads_the_catalogue_in_the_agreed_shape() {
    let shelf = Arc::new(Shelf::default());
    let rest = format!("&q=http&project={}", Uuid::from_u128(OURS));

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(&catalogue_of(MINE, &rest), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        found,
        serde_json::json!({"metrics": [
            {
                "name": ROUTE_METRIC,
                "kind": "sum",
                "unit": "{request}",
                "description": "about http.server/requests",
                "last_seen_ns": 1_700_000_001_000_000_000u64,
            },
            {
                "name": "http.server.duration",
                "kind": "histogram",
                "unit": "ms",
                "description": "about http.server.duration",
                "last_seen_ns": 1_700_000_001_000_000_000u64,
            },
        ]})
    );
    assert_eq!(
        shelf.listed(),
        vec![(
            org_id(MINE),
            Some("http".to_owned()),
            Some(project_id(OURS))
        )]
    );
}

#[tokio::test]
async fn a_member_reads_a_series_in_the_agreed_shape() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(
            &series_of(MINE, ENCODED, "&group_by=http.route&group_by=host&agg=sum"),
            &Who::Session,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    let start = FROM_MS * 1_000_000;
    assert_eq!(
        found,
        serde_json::json!({
            "step_ns": 1_000_000_000u64,
            "series": [{
                "labels": {"http.route": "/pay"},
                "points": [[start, 2.5], [start + 1_000_000_000, null]],
            }],
            "exemplars": [{
                "time_ns": start + 7,
                "value": 12.5,
                "trace_id": TRACE,
                "span_id": SPAN,
            }],
        })
    );
    let (org, kind, aggregation, query) = &shelf.charted()[0];
    assert_eq!(*org, org_id(MINE));
    assert_eq!(*kind, MetricKind::Sum);
    assert_eq!(*aggregation, Aggregation::Sum);
    assert_eq!(&*query.name, ROUTE_METRIC);
    assert_eq!(
        query.group_by,
        vec![Box::from("http.route"), Box::from("host")]
    );
    assert_eq!(query.window.from_unix_nanos(), start);
    assert_eq!(query.window.to_unix_nanos(), TO_MS * 1_000_000);
    assert_eq!(query.project, None);
}

#[tokio::test]
async fn a_series_without_an_aggregation_takes_the_default_of_its_kind() {
    let shelf = Arc::new(Shelf::default());

    let (status, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(MINE, "http.server.duration", ""), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(shelf.charted()[0].2, Aggregation::P95);
}

#[tokio::test]
async fn an_unknown_metric_is_not_found() {
    let shelf = Arc::new(Shelf::default());

    let (status, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(MINE, "nothing.here", ""), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(shelf.charted().is_empty());
}

#[tokio::test]
async fn an_aggregation_that_does_not_fit_the_kind_is_unprocessable() {
    let shelf = Arc::new(Shelf::default());

    let (quantile, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(MINE, "queue.depth", "&agg=p99"), &Who::Session),
    )
    .await;
    let (rate, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(MINE, "queue.depth", "&agg=rate"), &Who::Session),
    )
    .await;
    let many = "&group_by=k".repeat(9);
    let (grouped, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(MINE, "queue.depth", &many), &Who::Session),
    )
    .await;

    assert_eq!(quantile, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rate, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(grouped, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(shelf.charted().is_empty());
}

#[tokio::test]
async fn an_organisation_this_session_does_not_belong_to_is_not_found() {
    let shelf = Arc::new(Shelf::default());

    let (listed, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&catalogue_of(THEIRS, ""), &Who::Session),
    )
    .await;
    let (charted, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(THEIRS, ENCODED, ""), &Who::Session),
    )
    .await;

    assert_eq!(listed, StatusCode::NOT_FOUND);
    assert_eq!(charted, StatusCode::NOT_FOUND);
    assert!(shelf.listed().is_empty());
}

#[tokio::test]
async fn a_read_key_reads_the_metrics_of_its_organisation() {
    let shelf = Arc::new(Shelf::default());

    let (listed, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&catalogue_of(MINE, ""), &Who::Key(ORG_KEY)),
    )
    .await;
    let (charted, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(MINE, ENCODED, ""), &Who::Key(ORG_KEY)),
    )
    .await;
    let (stranger, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&catalogue_of(THEIRS, ""), &Who::Key(ORG_KEY)),
    )
    .await;

    assert_eq!((listed, charted), (StatusCode::OK, StatusCode::OK));
    assert_eq!(stranger, StatusCode::NOT_FOUND);
    assert_eq!(shelf.charted()[0].3.project, None);
}

#[tokio::test]
async fn a_project_bound_read_key_reads_only_its_project() {
    let shelf = Arc::new(Shelf::default());
    let other = format!("&project={}", Uuid::from_u128(OTHER));

    let (listed, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&catalogue_of(MINE, ""), &Who::Key(PROJECT_KEY)),
    )
    .await;
    let (charted, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(MINE, ENCODED, ""), &Who::Key(PROJECT_KEY)),
    )
    .await;
    let (elsewhere, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(MINE, ENCODED, &other), &Who::Key(PROJECT_KEY)),
    )
    .await;

    assert_eq!((listed, charted), (StatusCode::OK, StatusCode::OK));
    assert_eq!(elsewhere, StatusCode::NOT_FOUND);
    assert!(
        shelf
            .listed()
            .iter()
            .all(|(_, _, project)| *project == Some(project_id(OURS)))
    );
    assert_eq!(shelf.charted().len(), 1);
    assert_eq!(shelf.charted()[0].3.project, Some(project_id(OURS)));
}

#[tokio::test]
async fn a_reader_nobody_vouches_for_is_refused() {
    let shelf = Arc::new(Shelf::default());

    let (nobody, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&catalogue_of(MINE, ""), &Who::Nobody),
    )
    .await;
    let (unknown, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&series_of(MINE, ENCODED, ""), &Who::Key("ft_in_ingest")),
    )
    .await;

    assert_eq!(nobody, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown, StatusCode::UNAUTHORIZED);
    assert!(shelf.listed().is_empty());
}

#[tokio::test]
async fn malformed_parameters_are_refused_before_the_store_is_asked() {
    let shelf = Arc::new(Shelf::default());
    let mine = Uuid::from_u128(MINE);
    let cases = [
        (
            format!("/api/orgs/{mine}/metrics/{ENCODED}/series"),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("/api/orgs/{mine}/metrics/{ENCODED}/series?from=20&to=10"),
            StatusCode::BAD_REQUEST,
        ),
        (
            series_of(MINE, ENCODED, "&agg=median"),
            StatusCode::BAD_REQUEST,
        ),
        (
            series_of(MINE, ENCODED, "&project=nope"),
            StatusCode::BAD_REQUEST,
        ),
        (catalogue_of(MINE, "&project=nope"), StatusCode::BAD_REQUEST),
        (
            series_of(MINE, ENCODED, "&pod=a%20b"),
            StatusCode::BAD_REQUEST,
        ),
        (
            series_of(MINE, ENCODED, "&workload=replicaset:api"),
            StatusCode::BAD_REQUEST,
        ),
    ];

    for (uri, expected) in cases {
        let (status, body) = served(Some(Arc::clone(&shelf)), looking(&uri, &Who::Session)).await;
        assert_eq!(status, expected, "{uri}: {body}");
    }
    assert!(shelf.listed().is_empty());
}

#[tokio::test]
async fn without_a_metric_store_the_metrics_are_unavailable() {
    let (listed, _) = served(None, looking(&catalogue_of(MINE, ""), &Who::Session)).await;
    let (charted, _) = served(None, looking(&series_of(MINE, ENCODED, ""), &Who::Session)).await;

    assert_eq!(listed, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(charted, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn the_metric_routes_only_read() {
    let shelf = Arc::new(Shelf::default());
    let request = Request::builder()
        .method("POST")
        .uri(catalogue_of(MINE, ""))
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .expect("a request");

    let (status, _) = served(Some(Arc::clone(&shelf)), request).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(shelf.listed().is_empty());
}

#[tokio::test]
async fn a_placement_reaches_the_metric_store() {
    let shelf = Arc::new(Shelf::default());
    let uri = series_of(
        MINE,
        ENCODED,
        "&namespace=shop&workload=deployment:checkout&pod=checkout-1",
    );

    let (status, body) = served(Some(Arc::clone(&shelf)), looking(&uri, &Who::Session)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let charted = shelf.charted();
    let placement = &charted[0].3.placement;
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
        Some("deployment:checkout")
    );
    assert_eq!(
        placement
            .pod
            .as_ref()
            .map(ferristrace_domain::PlacementName::as_str),
        Some("checkout-1")
    );
    assert_eq!(placement.cluster, None);
    assert_eq!(placement.node, None);
}

fn charting_post(name: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!(
            "/api/orgs/{}/metrics/{name}/series",
            Uuid::from_u128(MINE)
        ))
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .expect("a request")
}

#[tokio::test]
async fn a_query_tree_narrows_the_series_the_store_is_asked_for() {
    let shelf = Arc::new(Shelf::default());
    let body = format!(
        r#"{{"from":{FROM_MS},"to":{TO_MS},"agg":"rate","group_by":["http.route"],
            "query":{{"all":[
              {{"field":"service","op":"eq","value":"checkout"}},
              {{"not":{{"attr":"http.route","op":"eq","value":"/health"}}}}]}}}}"#
    );

    let (status, text) = served(Some(Arc::clone(&shelf)), charting_post(ENCODED, &body)).await;

    assert_eq!(status, StatusCode::OK, "{text}");
    let (_, _, aggregation, query) = &shelf.charted()[0];
    assert_eq!(*aggregation, Aggregation::Rate);
    assert_eq!(query.group_by, vec!["http.route".into()]);
    assert!(!query.filter.is_anything());
}

#[tokio::test]
async fn a_span_only_field_is_refused_on_a_metric() {
    let shelf = Arc::new(Shelf::default());
    let body = format!(
        r#"{{"from":{FROM_MS},"to":{TO_MS},"query":{{"field":"status","op":"eq","value":"error"}}}}"#
    );

    let (status, _) = served(Some(Arc::clone(&shelf)), charting_post(ENCODED, &body)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(shelf.charted().is_empty());
}

#[tokio::test]
async fn the_attribute_routes_answer_with_the_shape_of_the_other_signals() {
    let shelf = Arc::new(Shelf::default());
    let uri = format!(
        "/api/orgs/{}/metrics/{ENCODED}/attributes?from={FROM_MS}&to={TO_MS}",
        Uuid::from_u128(MINE)
    );

    let (status, text) = served(Some(shelf), looking(&uri, &Who::Session)).await;

    assert_eq!(status, StatusCode::OK, "{text}");
    let found: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(found, serde_json::json!({"attributes": []}));
}
