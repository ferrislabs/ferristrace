use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, explore_router, issue_router, key_router, router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Ingestion, Stack};
use ferristrace_domain::{
    AccountId, Agent, ApiKey, ApiKeys, ApiSecret, Assigned, Counted, Created, Fingerprint,
    IngestKey, IssueDecisions, IssueFilter, IssueReader, IssuedKey, KeyId, KeyName, KeyScope,
    KeyScopes, LiveSession, Membership, Memberships, NoPhotos, Occurrence, OrgRole, Project,
    ProjectId, ProjectName, Projects, ProvisionPolicy, ReadGrant, Recorded, Renamed, Scope,
    ServiceFilter, ServiceSeries, ServiceSummary, Session, SessionError, SessionId, SessionRoll,
    SessionStore, SessionToken, Span, SpanCursor, SpanPage, SpanPages, SpanReader, Status,
    StoreError, TeamId, TenantDirectory, TenantId, TraceFilter, TraceId, TraceSummary, UnixSeconds,
    UnknownKey, WhoamiError, Window,
};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use prost::Message;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const STOREFRONT: u128 = 7;
const ELSEWHERE: u128 = 8;
const PRINT: &str = "0102030405060708090a0b0c0d0e0f10";

fn id(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

fn a_project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
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

struct MemberOfMine(OrgRole);

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: TenantId::new(Uuid::from_u128(MINE)),
            name: "Acme".into(),
            role: self.0,
        }])
    }
}

struct Held {
    org: TenantId,
    key: ApiKey,
    plaintext: String,
    revoked: bool,
}

#[derive(Default)]
struct Vault {
    held: Mutex<Vec<Held>>,
}

impl Vault {
    fn live(&self, plaintext: &str, scope: KeyScope) -> Option<(TenantId, Option<ProjectId>)> {
        self.held
            .lock()
            .expect("the lock")
            .iter()
            .find(|held| {
                held.plaintext == plaintext && !held.revoked && held.key.scopes.contains(scope)
            })
            .map(|held| (held.org, held.key.project))
    }
}

impl ApiKeys for Vault {
    async fn list(&self, org: TenantId) -> Result<Vec<ApiKey>, StoreError> {
        Ok(self
            .held
            .lock()
            .expect("the lock")
            .iter()
            .filter(|held| held.org == org && !held.revoked)
            .map(|held| held.key.clone())
            .collect())
    }

    async fn issue(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        by: AccountId,
    ) -> Result<IssuedKey, StoreError> {
        let mut held = self.held.lock().expect("the lock");
        let seed = u128::try_from(held.len()).expect("a count") + 100;
        let prefix = format!("ft_live_{seed:012x}");
        let plaintext = format!("{prefix}.{seed:064x}");
        let key = ApiKey {
            id: KeyId::new(Uuid::from_u128(seed)),
            project,
            name: name.as_str().into(),
            prefix: prefix.into(),
            scopes: scopes.clone(),
            created_at_unix_nanos: 1_700_000_000_123_456_789,
            created_by: Some(by),
            last_used_at_unix_nanos: None,
        };
        held.push(Held {
            org,
            key: key.clone(),
            plaintext: plaintext.clone(),
            revoked: false,
        });
        Ok(IssuedKey {
            key,
            secret: ApiSecret::from_plaintext(&plaintext),
        })
    }

    async fn revoke(&self, org: TenantId, id: KeyId) -> Result<bool, StoreError> {
        let mut held = self.held.lock().expect("the lock");
        Ok(held
            .iter_mut()
            .find(|held| held.org == org && held.key.id == id && !held.revoked)
            .map(|held| held.revoked = true)
            .is_some())
    }

    async fn grant_for(&self, secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(self
            .live(secret.exposed(), KeyScope::Read)
            .map(|(org, project)| ReadGrant { org, project }))
    }
}

struct Door(Arc<Vault>);

impl TenantDirectory for Door {
    async fn resolve(&self, key: &IngestKey) -> Result<Scope, UnknownKey> {
        self.0
            .live(key.exposed(), KeyScope::Ingest)
            .map(|(org, project)| Scope::of(org).in_project(project))
            .ok_or(UnknownKey)
    }
}

struct Storefront;

impl Projects for Storefront {
    async fn of(&self, tenant: TenantId) -> Result<Vec<Project>, StoreError> {
        Ok(vec![Project {
            id: a_project(STOREFRONT),
            org: tenant,
            name: "storefront".into(),
            created_at_unix_nanos: 1,
            team: None,
            keys: 0,
        }])
    }

    async fn create(&self, _org: TenantId, _name: &ProjectName) -> Result<Created, StoreError> {
        Ok(Created::Done)
    }

    async fn rename(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _name: &ProjectName,
    ) -> Result<Renamed, StoreError> {
        Ok(Renamed::Gone)
    }

    async fn forget(&self, _org: TenantId, _project: ProjectId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn assign(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _team: Option<TeamId>,
    ) -> Result<Assigned, StoreError> {
        Ok(Assigned::Done)
    }
}

#[derive(Clone, Default)]
struct Looked(Arc<Mutex<Vec<Option<ProjectId>>>>);

impl Looked {
    fn asked(&self) -> Vec<Option<ProjectId>> {
        self.0.lock().expect("the lock").clone()
    }

    fn note(&self, project: Option<ProjectId>) {
        self.0.lock().expect("the lock").push(project);
    }
}

impl SpanPages for Looked {
    async fn page(
        &self,
        _org: TenantId,
        _after: Option<&SpanCursor>,
        _limit: u32,
    ) -> Result<SpanPage, StoreError> {
        Ok(SpanPage::default())
    }
}

impl ferristrace_domain::LlmReader for Looked {
    async fn calls(
        &self,
        _org: ferristrace_domain::TenantId,
        _query: &ferristrace_domain::CallQuery,
    ) -> Result<ferristrace_domain::CallPage, ferristrace_domain::StoreError> {
        Ok(ferristrace_domain::CallPage::default())
    }

    async fn spend(
        &self,
        _org: ferristrace_domain::TenantId,
        _query: &ferristrace_domain::SpendQuery,
    ) -> Result<ferristrace_domain::SpendRead, ferristrace_domain::StoreError> {
        Ok(ferristrace_domain::SpendRead::default())
    }

    async fn models(
        &self,
        _org: ferristrace_domain::TenantId,
        _window: ferristrace_domain::Window,
        _project: Option<ferristrace_domain::ProjectId>,
    ) -> Result<Vec<ferristrace_domain::SeenModel>, ferristrace_domain::StoreError> {
        Ok(Vec::new())
    }
}

impl ferristrace_domain::ReleaseReader for Looked {
    async fn releases(
        &self,
        _org: TenantId,
        filter: &ferristrace_domain::ReleaseFilter,
    ) -> Result<Vec<ferristrace_domain::Release>, StoreError> {
        self.note(filter.project);
        Ok(Vec::new())
    }

    async fn release(
        &self,
        _org: TenantId,
        _service: &str,
        _version: &str,
        project: Option<ProjectId>,
    ) -> Result<Option<ferristrace_domain::ReleaseDetail>, StoreError> {
        self.note(project);
        Ok(None)
    }

    async fn deploys(
        &self,
        _org: TenantId,
        filter: &ferristrace_domain::ReleaseFilter,
    ) -> Result<Vec<ferristrace_domain::Deploy>, StoreError> {
        self.note(filter.project);
        Ok(Vec::new())
    }
}

impl ferristrace_domain::DeployFeed for Looked {
    async fn deployed(
        &self,
        _window: Window,
    ) -> Result<Vec<ferristrace_domain::Deployed>, StoreError> {
        Ok(Vec::new())
    }
}

impl ferristrace_domain::PlacementReader for Looked {
    async fn placements(
        &self,
        _tenant: TenantId,
        _window: ferristrace_domain::Window,
        project: Option<ProjectId>,
    ) -> Result<ferristrace_domain::Placements, StoreError> {
        self.note(project);
        Ok(ferristrace_domain::Placements::default())
    }
}

impl ferristrace_domain::CulpritReader for Looked {
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

impl SpanReader for Looked {
    async fn services(
        &self,
        _tenant: TenantId,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        self.note(filter.project);
        Ok(Vec::new())
    }

    async fn operations(
        &self,
        _tenant: TenantId,
        _service: &str,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        self.note(filter.project);
        Ok(Vec::new())
    }

    async fn series(
        &self,
        _tenant: TenantId,
        _service: Option<&str>,
        filter: &ServiceFilter,
    ) -> Result<ServiceSeries, StoreError> {
        self.note(filter.project);
        Ok(ServiceSeries::default())
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        self.note(filter.project);
        Ok(Vec::new())
    }

    async fn trace(
        &self,
        _tenant: TenantId,
        _id: TraceId,
        _project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        self.note(None);
        Ok(Vec::new())
    }
}

impl IssueReader for Looked {
    async fn issues(
        &self,
        _tenant: TenantId,
        filter: &IssueFilter,
    ) -> Result<Vec<Counted>, StoreError> {
        self.note(filter.project);
        Ok(Vec::new())
    }

    async fn occurrences(
        &self,
        _tenant: TenantId,
        project: ProjectId,
        _print: Fingerprint,
        _window: Window,
        _limit: u32,
    ) -> Result<Vec<Occurrence>, StoreError> {
        self.note(Some(project));
        Ok(Vec::new())
    }
}

struct Fakes {
    role: OrgRole,
    vault: Arc<Vault>,
}

impl Untransacted for Fakes {}

impl RepoFor<domain::Audit> for Fakes {
    type Repo = Inert;

    fn build(&self, (): &()) -> Self::Repo {
        Inert::default()
    }
}

impl RepoFor<domain::Sessions> for Fakes {
    type Repo = OneSession;

    fn build(&self, (): &()) -> Self::Repo {
        OneSession
    }
}

impl RepoFor<domain::Memberships> for Fakes {
    type Repo = MemberOfMine;

    fn build(&self, (): &()) -> Self::Repo {
        MemberOfMine(self.role)
    }
}

impl RepoFor<domain::ApiKeys> for Fakes {
    type Repo = Arc<Vault>;

    fn build(&self, (): &()) -> Self::Repo {
        Arc::clone(&self.vault)
    }
}

impl RepoFor<domain::Projects> for Fakes {
    type Repo = Storefront;

    fn build(&self, (): &()) -> Self::Repo {
        Storefront
    }
}

struct NoDecisions;

impl IssueDecisions for NoDecisions {
    async fn read_many(
        &self,
        _of: &[(ProjectId, Fingerprint)],
    ) -> Result<Vec<Recorded>, StoreError> {
        Ok(Vec::new())
    }

    async fn decide(
        &self,
        _project: ProjectId,
        _print: Fingerprint,
        _status: Status,
        _by: AccountId,
        _at_unix_nanos: u64,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    async fn assign(
        &self,
        _project: ProjectId,
        _print: Fingerprint,
        _owner: Option<AccountId>,
        _by: AccountId,
        _at_unix_nanos: u64,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

impl RepoFor<domain::Decisions> for Fakes {
    type Repo = NoDecisions;

    fn build(&self, (): &()) -> Self::Repo {
        NoDecisions
    }
}

struct TestStack;

impl Stack for TestStack {
    type Repos = Fakes;
    type Clock = Inert;
    type Hasher = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Looked;
    type IssueStore = Looked;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Oidc = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Door;
    type Webhooks = Inert;
}

struct Harness {
    vault: Arc<Vault>,
    looked: Looked,
}

impl Harness {
    fn new() -> Self {
        Self {
            vault: Arc::default(),
            looked: Looked::default(),
        }
    }

    fn app(&self, role: OrgRole) -> Router {
        let adapters = Adapters {
            clock: Inert::default(),
            hasher: Inert::default(),
            mailer: Inert::default(),
            webhooks: Inert::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: Some(self.looked.clone()),
            issue_store: Some(self.looked.clone()),
            log_store: None,
            metric_store: None,
            oidc: None,
            provision: ProvisionPolicy::default(),
        };
        let state = AppState::new(
            Arc::new(Ferristrace::<TestStack>::new(
                Fakes {
                    role,
                    vault: Arc::clone(&self.vault),
                },
                adapters,
            )),
            SessionCookie::default(),
        );
        let ingest = Arc::new(Ingestion::<TestStack>::new(
            Door(Arc::clone(&self.vault)),
            Inert::default(),
            Inert::default(),
            Inert::default(),
        ));
        key_router(state.clone())
            .merge(explore_router(state.clone()))
            .merge(issue_router(state))
            .merge(router(ingest))
    }

    async fn served(&self, role: OrgRole, request: Request<Body>) -> (StatusCode, String) {
        let response = self.app(role).oneshot(request).await.expect("a response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, String::from_utf8(body.to_vec()).expect("text"))
    }

    async fn issued(&self, body: &str) -> serde_json::Value {
        let (status, body) = self
            .served(
                OrgRole::Admin,
                as_session(Method::POST, &keys_of(MINE), Some(body)),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        read(&body)
    }
}

fn keys_of(org: u128) -> String {
    format!("/api/orgs/{}/keys", id(org))
}

fn as_session(method: Method, uri: &str, body: Option<&str>) -> Request<Body> {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-requested-with", "fetch")
        .header(COOKIE, format!("ft_session={TOKEN}"));
    match body {
        Some(json) => request
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_owned())),
        None => request.body(Body::empty()),
    }
    .expect("a request")
}

fn as_key(method: Method, uri: &str, secret: &str, body: Option<&str>) -> Request<Body> {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {secret}"));
    match body {
        Some(json) => request
            .header(CONTENT_TYPE, "application/json")
            .header("x-requested-with", "fetch")
            .body(Body::from(json.to_owned())),
        None => request.body(Body::empty()),
    }
    .expect("a request")
}

fn an_export(secret: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/v1/traces")
        .header(CONTENT_TYPE, "application/x-protobuf")
        .header(AUTHORIZATION, format!("Bearer {secret}"))
        .body(Body::from(
            ExportTraceServiceRequest::default().encode_to_vec(),
        ))
        .expect("a request")
}

fn read(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("json")
}

fn secret_of(issued: &serde_json::Value) -> String {
    issued["secret"].as_str().expect("a secret").to_owned()
}

fn traces_of(org: u128) -> String {
    format!("/api/orgs/{}/traces?from=1&to=2", id(org))
}

#[tokio::test]
async fn a_read_key_is_refused_by_ingest_and_an_ingest_key_by_the_api() {
    let harness = Harness::new();
    let read_only = secret_of(
        &harness
            .issued(r#"{"name":"dashboards","scopes":["read"]}"#)
            .await,
    );
    let ingest_only = secret_of(
        &harness
            .issued(r#"{"name":"collector","scopes":["ingest"]}"#)
            .await,
    );

    let (read_ingesting, _) = harness.served(OrgRole::Member, an_export(&read_only)).await;
    let (ingest_ingesting, _) = harness
        .served(OrgRole::Member, an_export(&ingest_only))
        .await;
    let (ingest_reading, _) = harness
        .served(
            OrgRole::Member,
            as_key(Method::GET, &traces_of(MINE), &ingest_only, None),
        )
        .await;
    let (read_reading, _) = harness
        .served(
            OrgRole::Member,
            as_key(Method::GET, &traces_of(MINE), &read_only, None),
        )
        .await;

    assert_eq!(read_ingesting, StatusCode::UNAUTHORIZED);
    assert_eq!(ingest_ingesting, StatusCode::OK);
    assert_eq!(ingest_reading, StatusCode::UNAUTHORIZED);
    assert_eq!(
        read_reading,
        StatusCode::OK,
        "a key is not a browser and needs no x-requested-with"
    );
}

#[tokio::test]
async fn a_read_key_reads_the_nine_read_routes() {
    let harness = Harness::new();
    let secret = secret_of(
        &harness
            .issued(r#"{"name":"dashboards","scopes":["read"]}"#)
            .await,
    );
    let org = id(MINE);
    let project = id(STOREFRONT);
    let routes = [
        format!("/api/orgs/{org}/services?from=1&to=2"),
        format!("/api/orgs/{org}/traces?from=1&to=2"),
        format!("/api/orgs/{org}/traces/0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a"),
        format!("/api/orgs/{org}/issues?from=1&to=2"),
        format!("/api/orgs/{org}/issues/{PRINT}?from=1&to=2"),
        format!("/api/orgs/{org}/issues/{PRINT}/occurrences?from=1&to=2&project={project}"),
        format!("/api/orgs/{org}/services/checkout/operations?from=1&to=2"),
        format!("/api/orgs/{org}/services/checkout/series?from=1&to=2"),
        format!("/api/orgs/{org}/series?from=1&to=2"),
    ];

    for route in routes {
        let (status, body) = harness
            .served(OrgRole::Member, as_key(Method::GET, &route, &secret, None))
            .await;

        assert_ne!(status, StatusCode::UNAUTHORIZED, "{route}: {body}");
        assert_ne!(status, StatusCode::FORBIDDEN, "{route}: {body}");
    }
    assert_eq!(harness.looked.asked().len(), 9);
}

#[tokio::test]
async fn a_project_bound_read_key_gets_only_its_project_even_when_another_is_named() {
    let harness = Harness::new();
    let secret = secret_of(
        &harness
            .issued(&format!(
                r#"{{"name":"storefront","scopes":["read"],"project":"{}"}}"#,
                id(STOREFRONT)
            ))
            .await,
    );
    let elsewhere = id(ELSEWHERE);

    let (open, _) = harness
        .served(
            OrgRole::Member,
            as_key(Method::GET, &traces_of(MINE), &secret, None),
        )
        .await;
    let (traces, _) = harness
        .served(
            OrgRole::Member,
            as_key(
                Method::GET,
                &format!("{}&project={elsewhere}", traces_of(MINE)),
                &secret,
                None,
            ),
        )
        .await;
    let (issues, _) = harness
        .served(
            OrgRole::Member,
            as_key(
                Method::GET,
                &format!(
                    "/api/orgs/{}/issues?from=1&to=2&project={elsewhere}",
                    id(MINE)
                ),
                &secret,
                None,
            ),
        )
        .await;
    let (theirs, _) = harness
        .served(
            OrgRole::Member,
            as_key(Method::GET, &traces_of(THEIRS), &secret, None),
        )
        .await;

    assert_eq!(open, StatusCode::OK);
    assert_eq!(traces, StatusCode::NOT_FOUND);
    assert_eq!(issues, StatusCode::FORBIDDEN);
    assert_eq!(theirs, StatusCode::NOT_FOUND);
    assert_eq!(harness.looked.asked(), vec![Some(a_project(STOREFRONT))]);
}

#[tokio::test]
async fn the_secret_is_returned_at_creation_and_never_listed() {
    let harness = Harness::new();

    let issued = harness
        .issued(r#"{"name":" edge collector ","scopes":["ingest","read"]}"#)
        .await;
    let (status, listed) = harness
        .served(
            OrgRole::Member,
            as_session(Method::GET, &keys_of(MINE), None),
        )
        .await;

    let secret = secret_of(&issued);
    assert!(secret.starts_with(issued["key"]["prefix"].as_str().expect("a prefix")));
    assert_eq!(issued["key"]["name"], "edge collector");
    assert_eq!(
        issued["key"]["scopes"],
        serde_json::json!(["ingest", "read"])
    );
    assert_eq!(status, StatusCode::OK);
    assert!(!listed.contains(&secret), "{listed}");
    assert!(!listed.contains("secret"), "{listed}");
    let listed = read(&listed);
    assert_eq!(listed["keys"][0]["name"], "edge collector");
    assert_eq!(listed["keys"][0]["created_at_ms"], 1_700_000_000_123u64);
    assert_eq!(
        listed["keys"][0]["last_used_at_ms"],
        serde_json::Value::Null
    );
    assert_eq!(listed["keys"][0]["project"], serde_json::Value::Null);
}

#[tokio::test]
async fn a_member_lists_but_only_an_administrator_issues_or_revokes() {
    let harness = Harness::new();
    let issued = harness
        .issued(r#"{"name":"collector","scopes":["ingest","read"]}"#)
        .await;
    let one = format!(
        "{}/{}",
        keys_of(MINE),
        issued["key"]["id"].as_str().expect("an id")
    );

    let (listed, _) = harness
        .served(
            OrgRole::Member,
            as_session(Method::GET, &keys_of(MINE), None),
        )
        .await;
    let (created, _) = harness
        .served(
            OrgRole::Member,
            as_session(
                Method::POST,
                &keys_of(MINE),
                Some(r#"{"name":"mine","scopes":["read"]}"#),
            ),
        )
        .await;
    let (revoked_by_member, _) = harness
        .served(OrgRole::Member, as_session(Method::DELETE, &one, None))
        .await;
    let (revoked_by_owner, _) = harness
        .served(OrgRole::Owner, as_session(Method::DELETE, &one, None))
        .await;
    let (again, _) = harness
        .served(OrgRole::Owner, as_session(Method::DELETE, &one, None))
        .await;

    assert_eq!(listed, StatusCode::OK);
    assert_eq!(created, StatusCode::FORBIDDEN);
    assert_eq!(revoked_by_member, StatusCode::FORBIDDEN);
    assert_eq!(revoked_by_owner, StatusCode::NO_CONTENT);
    assert_eq!(again, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_revoked_key_is_refused_at_once() {
    let harness = Harness::new();
    let issued = harness
        .issued(r#"{"name":"collector","scopes":["ingest","read"]}"#)
        .await;
    let secret = secret_of(&issued);
    let one = format!(
        "{}/{}",
        keys_of(MINE),
        issued["key"]["id"].as_str().expect("an id")
    );

    let (deleted, _) = harness
        .served(OrgRole::Admin, as_session(Method::DELETE, &one, None))
        .await;
    let (ingesting, _) = harness.served(OrgRole::Member, an_export(&secret)).await;
    let (reading, _) = harness
        .served(
            OrgRole::Member,
            as_key(Method::GET, &traces_of(MINE), &secret, None),
        )
        .await;

    assert_eq!(deleted, StatusCode::NO_CONTENT);
    assert_eq!(ingesting, StatusCode::UNAUTHORIZED);
    assert_eq!(reading, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_read_key_writes_nothing() {
    let harness = Harness::new();
    let secret = secret_of(
        &harness
            .issued(r#"{"name":"dashboards","scopes":["read","ingest"]}"#)
            .await,
    );
    let decide = format!("/api/orgs/{}/issues/{PRINT}", id(MINE));
    let decision = format!(r#"{{"project":"{}","status":"resolved"}}"#, id(STOREFRONT));

    let (ruled, _) = harness
        .served(
            OrgRole::Owner,
            as_key(Method::PATCH, &decide, &secret, Some(&decision)),
        )
        .await;
    let (issued, _) = harness
        .served(
            OrgRole::Owner,
            as_key(
                Method::POST,
                &keys_of(MINE),
                &secret,
                Some(r#"{"name":"more","scopes":["read"]}"#),
            ),
        )
        .await;
    let (listed, _) = harness
        .served(
            OrgRole::Owner,
            as_key(Method::GET, &keys_of(MINE), &secret, None),
        )
        .await;

    assert_eq!(ruled, StatusCode::UNAUTHORIZED);
    assert_eq!(issued, StatusCode::UNAUTHORIZED);
    assert_eq!(
        listed,
        StatusCode::FORBIDDEN,
        "the key routes are for a browser session only"
    );
}

#[tokio::test]
async fn a_request_from_another_origin_never_reaches_the_keys() {
    let harness = Harness::new();
    let request = Request::builder()
        .uri(keys_of(MINE))
        .header(COOKIE, format!("ft_session={TOKEN}"))
        .body(Body::empty())
        .expect("a request");

    let (status, _) = harness.served(OrgRole::Owner, request).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_keys_of_an_organisation_this_session_does_not_belong_to_are_not_found() {
    let harness = Harness::new();

    let (status, _) = harness
        .served(
            OrgRole::Owner,
            as_session(Method::GET, &keys_of(THEIRS), None),
        )
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_bad_name_bad_scopes_or_a_foreign_project_are_refused() {
    let harness = Harness::new();
    let foreign = format!(
        r#"{{"name":"ci","scopes":["read"],"project":"{}"}}"#,
        id(ELSEWHERE)
    );

    let mut answered = Vec::new();
    for body in [
        r#"{"name":"  ","scopes":["read"]}"#,
        r#"{"name":"ci","scopes":[]}"#,
        r#"{"name":"ci","scopes":["read","read"]}"#,
        r#"{"name":"ci","scopes":["admin"]}"#,
        foreign.as_str(),
    ] {
        let (status, _) = harness
            .served(
                OrgRole::Owner,
                as_session(Method::POST, &keys_of(MINE), Some(body)),
            )
            .await;
        answered.push(status);
    }

    assert_eq!(
        answered,
        vec![
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::NOT_FOUND,
        ]
    );
    assert_eq!(
        harness
            .vault
            .list(TenantId::new(Uuid::from_u128(MINE)))
            .await,
        Ok(Vec::new())
    );
}

#[tokio::test]
async fn a_project_bound_read_key_reads_operations_and_series_of_its_project_only() {
    let harness = Harness::new();
    let secret = secret_of(
        &harness
            .issued(&format!(
                r#"{{"name":"storefront","scopes":["read"],"project":"{}"}}"#,
                id(STOREFRONT)
            ))
            .await,
    );
    let org = id(MINE);
    let elsewhere = id(ELSEWHERE);
    let mut statuses = Vec::new();
    for route in [
        format!("/api/orgs/{org}/services/checkout/operations?from=1&to=2"),
        format!("/api/orgs/{org}/series?from=1&to=2"),
        format!("/api/orgs/{org}/services/checkout/operations?from=1&to=2&project={elsewhere}"),
        format!("/api/orgs/{org}/services/checkout/series?from=1&to=2&project={elsewhere}"),
    ] {
        let (status, _) = harness
            .served(OrgRole::Member, as_key(Method::GET, &route, &secret, None))
            .await;
        statuses.push(status);
    }

    assert_eq!(
        statuses,
        vec![
            StatusCode::OK,
            StatusCode::OK,
            StatusCode::NOT_FOUND,
            StatusCode::NOT_FOUND
        ]
    );
    assert_eq!(
        harness.looked.asked(),
        vec![Some(a_project(STOREFRONT)), Some(a_project(STOREFRONT))]
    );
}

#[tokio::test]
async fn a_project_bound_read_key_lists_the_placements_of_its_project_only() {
    let harness = Harness::new();
    let secret = secret_of(
        &harness
            .issued(&format!(
                r#"{{"name":"storefront","scopes":["read"],"project":"{}"}}"#,
                id(STOREFRONT)
            ))
            .await,
    );
    let org = id(MINE);
    let elsewhere = id(ELSEWHERE);
    let mut statuses = Vec::new();
    for route in [
        format!("/api/orgs/{org}/placements?from=1&to=2"),
        format!("/api/orgs/{org}/placements?from=1&to=2&project={elsewhere}"),
        format!("/api/orgs/{}/placements?from=1&to=2", id(THEIRS)),
    ] {
        let (status, _) = harness
            .served(OrgRole::Member, as_key(Method::GET, &route, &secret, None))
            .await;
        statuses.push(status);
    }

    assert_eq!(
        statuses,
        vec![StatusCode::OK, StatusCode::NOT_FOUND, StatusCode::NOT_FOUND]
    );
    assert_eq!(harness.looked.asked(), vec![Some(a_project(STOREFRONT))]);
}
