use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use ferristrace_api::{AppState, infra_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Aggregation, ApiKey, ApiKeys, ApiSecret, ClusterFilter, ClusterReader,
    ClusterSnapshot, InfraFilter, InfraReader, IssuedKey, KeyId, KeyName, KeyScopes, KeyTtl,
    Membership, Memberships, MetricDescriptor, MetricKind, MetricQuery, MetricReader, NoPhotos,
    OrgRole, ProjectId, ReadGrant, Resource, ResourceKind, SeriesSet, Session, SessionError,
    SessionStore, SessionToken, StoreError, TenantId, UnixSeconds, WhoamiError, Window,
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
        _ttl: KeyTtl,
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

type Asked = (TenantId, Window, InfraFilter);

#[derive(Debug, Default)]
struct Shelf {
    asked: Mutex<Vec<Asked>>,
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

fn resource(kind: ResourceKind, name: &str, last_seen_secs: u64) -> Resource {
    Resource {
        kind,
        id: name.into(),
        name: name.into(),
        cluster: "".into(),
        environment: "prod".into(),
        provider: "aws".into(),
        region: "eu-west-1".into(),
        zone: "".into(),
        host_type: "".into(),
        first_seen_unix_nanos: 1_700_000_000 * SECOND,
        last_seen_unix_nanos: last_seen_secs * SECOND,
        silent: false,
        cpu_percent: Some(12.5),
        memory_bytes: Some(2048),
        services: vec!["checkout".into(), "mailer".into()],
    }
}

impl InfraReader for Store {
    async fn resources(
        &self,
        org: TenantId,
        figures: Window,
        filter: &InfraFilter,
    ) -> Result<Vec<Resource>, StoreError> {
        self.0
            .asked
            .lock()
            .expect("the lock")
            .push((org, figures, filter.clone()));
        Ok(vec![
            resource(ResourceKind::Cluster, "prod-eu", NOW - 10),
            resource(ResourceKind::Container, "web", NOW - 20),
            resource(ResourceKind::Host, "gone", NOW - 900),
        ])
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
    let response = infra_router(state(shelf))
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

fn infra_of(seed: u128, rest: &str) -> String {
    format!(
        "/api/orgs/{}/infrastructure?x=1{rest}",
        Uuid::from_u128(seed)
    )
}

#[tokio::test]
async fn a_member_lists_every_resource_with_its_silence_settled() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(shelf.clone()),
        looking(&infra_of(MINE, ""), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("json");
    let listed = json["resources"].as_array().expect("a list");
    assert_eq!(listed.len(), 3, "{body}");
    assert_eq!(listed[0]["kind"], "cluster");
    assert_eq!(listed[0]["silent"], false);
    assert_eq!(listed[1]["cpu_percent"], 12.5);
    assert_eq!(listed[1]["provider"], "aws");
    assert_eq!(
        listed[1]["services"],
        serde_json::json!(["checkout", "mailer"])
    );
    assert_eq!(listed[2]["silent"], true, "{body}");
    assert!(
        listed[2]["cpu_percent"].is_null(),
        "a silent resource shows no figures"
    );
    assert!(listed[2]["memory_bytes"].is_null());
    assert!(listed[0]["cluster"].is_null(), "blank facets are null");
}

#[tokio::test]
async fn the_filters_reach_the_reader() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(shelf.clone()),
        looking(
            &infra_of(
                MINE,
                "&kind=host&cluster=prod-eu&environment=prod&provider=aws&region=eu-west-1",
            ),
            &Who::Session,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let asked = shelf.asked();
    assert_eq!(asked.len(), 1);
    let (org, _, filter) = &asked[0];
    assert_eq!(*org, org_id(MINE));
    assert_eq!(filter.kind, Some(ResourceKind::Host));
    assert_eq!(
        filter.cluster.as_ref().map(ToString::to_string).as_deref(),
        Some("prod-eu")
    );
    assert_eq!(
        filter.region.as_ref().map(ToString::to_string).as_deref(),
        Some("eu-west-1")
    );
}

#[tokio::test]
async fn the_status_filter_keeps_only_silent_or_only_active_resources() {
    let (_, body) = served(
        Some(Arc::new(Shelf::default())),
        looking(&infra_of(MINE, "&status=silent"), &Who::Session),
    )
    .await;
    let json: serde_json::Value = serde_json::from_str(&body).expect("json");
    let names: Vec<_> = json["resources"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|r| r["name"].as_str().expect("a name").to_owned())
        .collect();
    assert_eq!(names, ["gone"]);

    let (_, body) = served(
        Some(Arc::new(Shelf::default())),
        looking(&infra_of(MINE, "&status=active"), &Who::Session),
    )
    .await;
    let json: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(json["resources"].as_array().expect("a list").len(), 2);
}

#[tokio::test]
async fn a_hostile_or_unknown_filter_is_a_bad_request_before_any_read() {
    for rest in [
        "&kind=database",
        "&status=gone",
        "&cluster=a'%20OR%201=1",
        "&region=%3Cscript%3E",
    ] {
        let shelf = Arc::new(Shelf::default());

        let (status, _) = served(
            Some(shelf.clone()),
            looking(&infra_of(MINE, rest), &Who::Session),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{rest}");
        assert!(shelf.asked().is_empty(), "{rest} reached the store");
    }
}

#[tokio::test]
async fn a_project_key_is_narrowed_to_its_project_and_cannot_ask_for_another() {
    let shelf = Arc::new(Shelf::default());
    let (status, body) = served(
        Some(shelf.clone()),
        looking(&infra_of(MINE, ""), &Who::Key(PROJECT_KEY)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(shelf.asked()[0].2.project, Some(project_id(OURS)));

    let other = format!("&project={}", Uuid::from_u128(OTHER));
    let (status, _) = served(
        Some(Arc::new(Shelf::default())),
        looking(&infra_of(MINE, &other), &Who::Key(PROJECT_KEY)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn another_organisation_is_not_found_and_no_token_is_unauthorised() {
    let (status, _) = served(
        Some(Arc::new(Shelf::default())),
        looking(&infra_of(THEIRS, ""), &Who::Session),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = served(
        Some(Arc::new(Shelf::default())),
        looking(&infra_of(MINE, ""), &Who::Nobody),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn without_a_metric_store_the_answer_is_unavailable() {
    let (status, _) = served(None, looking(&infra_of(MINE, ""), &Who::Session)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
