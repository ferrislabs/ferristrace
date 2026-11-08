use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use ferristrace_api::{AppState, cluster_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Aggregation, ApiKey, ApiKeys, ApiSecret, ClusterFilter, ClusterReader,
    ClusterSnapshot, InfraFilter, InfraReader, IssuedKey, KeyId, KeyName, KeyScopes, Membership,
    Memberships, MetricDescriptor, MetricKind, MetricQuery, MetricReader, NoPhotos, Node, OrgRole,
    PlacementName, Pod, PodPhase, ProjectId, ReadGrant, Resource, SeriesSet, Session, SessionError,
    SessionStore, SessionToken, StoreError, TenantId, UnixSeconds, WhoamiError, Window,
    WorkloadKind, WorkloadSummary,
};
use ferristrace_infra::FixedClock;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft.person.0123456789";
const ORG_KEY: &str = "ft_rd_whole_org";
const PROJECT_KEY: &str = "ft_rd_one_project";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const OURS: u128 = 30;
const OTHER: u128 = 40;
const NOW: u64 = 1_700_000_600;
const SECOND: u64 = 1_000_000_000;

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

type Asked = (TenantId, Window, ClusterFilter);

#[derive(Debug, Default)]
struct Shelf {
    asked: Mutex<Vec<Asked>>,
    empty: bool,
}

impl Shelf {
    fn asked(&self) -> Vec<Asked> {
        self.asked.lock().expect("the lock").clone()
    }
}

struct Store(Arc<Shelf>);

impl MetricReader for Store {
    async fn catalogue(
        &self,
        _org: TenantId,
        _search: Option<&str>,
        _project: Option<ProjectId>,
    ) -> Result<Vec<MetricDescriptor>, StoreError> {
        Ok(Vec::new())
    }

    async fn series(
        &self,
        _org: TenantId,
        _kind: MetricKind,
        _aggregation: Aggregation,
        query: &MetricQuery,
    ) -> Result<SeriesSet, StoreError> {
        Ok(SeriesSet {
            step_nanos: query.step_nanos(),
            ..SeriesSet::default()
        })
    }
}

fn reported() -> ClusterSnapshot {
    ClusterSnapshot {
        clusters: vec!["prod".into(), "staging".into()],
        namespaces: vec!["shop".into()],
        nodes: vec![Node {
            ready: Some(true),
            cpu_cores: Some(1.5),
            memory_bytes: Some(4_000_000_000),
            memory_available: Some(8_000_000_000),
            ..Node::named("node-a")
        }],
        workloads: vec![WorkloadSummary {
            cluster: "prod".into(),
            kind: WorkloadKind::Deployment,
            name: "mailer".into(),
            namespace: "shop".into(),
            desired: Some(2),
            ready: Some(1),
            pods: 0,
        }],
        pods: vec![Pod {
            cluster: "prod".into(),
            workload: Some((WorkloadKind::Deployment, "mailer".into())),
            node: Some("node-a".into()),
            phase: Some(PodPhase::Running),
            restarts_in_window: 4,
            restarts_total: 9,
            cpu_cores: Some(0.25),
            memory_bytes: Some(200_000_000),
            ready: Some(false),
            services: vec!["mailer".into()],
            ..Pod::named("mailer-1", "shop")
        }],
        seen_ms: Some(1_700_000_590_000),
    }
}

impl ClusterReader for Store {
    async fn snapshot(
        &self,
        org: TenantId,
        window: Window,
        filter: &ClusterFilter,
    ) -> Result<ClusterSnapshot, StoreError> {
        self.0
            .asked
            .lock()
            .expect("the lock")
            .push((org, window, filter.clone()));
        if self.0.empty {
            return Ok(ClusterSnapshot::default());
        }
        Ok(reported())
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
    type Clock = FixedClock;
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
        clock: FixedClock::at(NOW),
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
    let response = cluster_router(state(shelf))
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

fn cluster_of(seed: u128, rest: &str) -> String {
    format!("/api/orgs/{}/cluster?x=1{rest}", Uuid::from_u128(seed))
}

#[tokio::test]
async fn a_member_reads_the_cluster_in_the_agreed_shape() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(&cluster_of(MINE, ""), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        found,
        serde_json::json!({
            "clusters": ["prod", "staging"],
            "namespaces": ["shop"],
            "nodes": [{
                "name": "node-a",
                "ready": true,
                "cpu": 1.5,
                "memory": 4_000_000_000u64,
                "memory_available": 8_000_000_000u64,
                "pods": 1,
            }],
            "workloads": [{
                "cluster": "prod",
                "kind": "deployment",
                "name": "mailer",
                "namespace": "shop",
                "desired": 2,
                "ready": 1,
                "pods": 1,
            }],
            "pods": [{
                "name": "mailer-1",
                "namespace": "shop",
                "cluster": "prod",
                "workload": {"kind": "deployment", "name": "mailer"},
                "node": "node-a",
                "phase": "Running",
                "ready": false,
                "restarts": 4,
                "restarts_total": 9,
                "cpu": 0.25,
                "memory": 200_000_000u64,
                "services": ["mailer"],
                "unhealthy": true,
                "reason": "restarting",
            }],
            "seen_ms": 1_700_000_590_000u64,
        })
    );
    let asked = shelf.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, org_id(MINE));
    assert_eq!(asked[0].1.to_unix_nanos(), (NOW + 1) * SECOND);
    assert_eq!(asked[0].1.from_unix_nanos(), (NOW - 600) * SECOND);
    assert_eq!(asked[0].2, ClusterFilter::default());
}

#[tokio::test]
async fn nothing_reporting_reads_as_empty_lists_for_the_setup_state() {
    let shelf = Arc::new(Shelf {
        empty: true,
        ..Shelf::default()
    });

    let (status, body) = served(Some(shelf), looking(&cluster_of(MINE, ""), &Who::Session)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        found,
        serde_json::json!({
            "clusters": [],
            "namespaces": [],
            "nodes": [],
            "workloads": [],
            "pods": [],
            "seen_ms": null,
        })
    );
}

#[tokio::test]
async fn the_cluster_namespace_and_project_reach_the_store() {
    let shelf = Arc::new(Shelf::default());
    let rest = format!(
        "&cluster=prod&namespace=%20shop%20&project={}",
        Uuid::from_u128(OTHER)
    );

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(&cluster_of(MINE, &rest), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let filter = &shelf.asked()[0].2;
    assert_eq!(
        filter.cluster.as_ref().map(PlacementName::as_str),
        Some("prod")
    );
    assert_eq!(
        filter.namespace.as_ref().map(PlacementName::as_str),
        Some("shop")
    );
    assert_eq!(filter.project, Some(project_id(OTHER)));
}

#[tokio::test]
async fn a_malformed_placement_is_a_bad_request_before_the_store_is_asked() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(&cluster_of(MINE, "&namespace=a%20b"), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "the namespace parameter is not a valid placement");
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn a_read_key_reads_the_cluster_and_a_project_key_only_its_project() {
    let shelf = Arc::new(Shelf::default());

    let (whole, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&cluster_of(MINE, ""), &Who::Key(ORG_KEY)),
    )
    .await;
    let (bound, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&cluster_of(MINE, ""), &Who::Key(PROJECT_KEY)),
    )
    .await;
    let (elsewhere, _) = served(
        Some(Arc::clone(&shelf)),
        looking(
            &cluster_of(MINE, &format!("&project={}", Uuid::from_u128(OTHER))),
            &Who::Key(PROJECT_KEY),
        ),
    )
    .await;

    assert_eq!(whole, StatusCode::OK);
    assert_eq!(bound, StatusCode::OK);
    assert_eq!(elsewhere, StatusCode::NOT_FOUND);
    assert_eq!(
        shelf
            .asked()
            .into_iter()
            .map(|(_, _, filter)| filter.project)
            .collect::<Vec<_>>(),
        vec![None, Some(project_id(OURS))]
    );
}

#[tokio::test]
async fn a_stranger_and_an_anonymous_caller_are_refused() {
    let shelf = Arc::new(Shelf::default());

    let (theirs, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&cluster_of(THEIRS, ""), &Who::Session),
    )
    .await;
    let (key_elsewhere, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&cluster_of(THEIRS, ""), &Who::Key(ORG_KEY)),
    )
    .await;
    let (nobody, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&cluster_of(MINE, ""), &Who::Nobody),
    )
    .await;

    assert_eq!(theirs, StatusCode::NOT_FOUND);
    assert_eq!(key_elsewhere, StatusCode::NOT_FOUND);
    assert_eq!(nobody, StatusCode::UNAUTHORIZED);
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn no_metric_store_is_unavailable() {
    let (status, _) = served(None, looking(&cluster_of(MINE, ""), &Who::Session)).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
