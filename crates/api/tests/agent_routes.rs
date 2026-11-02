use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{
    AppState, SessionCookie, agent_router, explore_router, key_router, mark_routes,
};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Notebook, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Action, Actor, Agent, AgentEnrolment, AgentId, AgentScope, AgentScopes, AgentToken,
    AgentTokens, Agents, ApiKey, ApiKeys, ApiSecret, Assigned, AuditEntry, AuditTrail, Counted,
    Created, Fingerprint, IssueFilter, IssueReader, IssuedKey, KeyId, KeyName, KeyScopes,
    LiveSession, Membership, Memberships, NoPhotos, Occurrence, OrgRole, Project, ProjectId,
    ProjectName, Projects, ProvisionPolicy, ReadGrant, RegisteredAgent, Renamed, ServiceFilter,
    ServiceSeries, ServiceSummary, Session, SessionError, SessionId, SessionRoll, SessionStore,
    SessionToken, Span, SpanCursor, SpanPage, SpanPages, SpanReader, StoreError, TeamId, TenantId,
    TokenRefusal, TraceFilter, TraceId, TraceSummary, UnixSeconds, VerifiedToken, WhoamiError,
    Window,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const STOREFRONT: u128 = 7;
const ELSEWHERE: u128 = 8;

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

#[derive(Default)]
struct Book {
    agents: Mutex<Vec<RegisteredAgent>>,
    notebook: Notebook,
}

impl Book {
    fn live(&self, org: TenantId, client_id: &str) -> Option<RegisteredAgent> {
        let agents = self.agents.lock().expect("the lock");
        let mut found: Vec<&RegisteredAgent> = agents
            .iter()
            .filter(|agent| agent.org == org && &*agent.client_id == client_id)
            .collect();
        found.sort_by_key(|agent| agent.is_revoked());
        found.first().map(|agent| (*agent).clone())
    }

    fn id_of(&self, client_id: &str) -> AgentId {
        self.live(TenantId::new(Uuid::from_u128(MINE)), client_id)
            .expect("registered")
            .id
    }
}

struct Roll(Arc<Book>);

impl Agents for Roll {
    async fn list(&self, org: TenantId) -> Result<Vec<RegisteredAgent>, StoreError> {
        Ok(self
            .0
            .agents
            .lock()
            .expect("the lock")
            .iter()
            .filter(|agent| agent.org == org)
            .cloned()
            .collect())
    }

    async fn register(
        &self,
        org: TenantId,
        enrolment: &AgentEnrolment,
        by: AccountId,
    ) -> Result<Option<RegisteredAgent>, StoreError> {
        let mut agents = self.0.agents.lock().expect("the lock");
        let taken = agents.iter().any(|agent| {
            agent.org == org
                && !agent.is_revoked()
                && &*agent.client_id == enrolment.client_id.as_str()
        });
        if taken {
            return Ok(None);
        }
        let agent = RegisteredAgent {
            id: AgentId::new(Uuid::from_u128(
                500 + u128::try_from(agents.len()).expect("a count"),
            )),
            org,
            name: enrolment.name.as_str().into(),
            client_id: enrolment.client_id.as_str().into(),
            scopes: enrolment.scopes.clone(),
            project: enrolment.project,
            created_by: Some(by),
            created_at_unix_nanos: 1_700_000_000_123_456_789,
            last_used_at_unix_nanos: None,
            revoked_at_unix_nanos: None,
        };
        agents.push(agent.clone());
        Ok(Some(agent))
    }

    async fn revoke(
        &self,
        org: TenantId,
        id: AgentId,
    ) -> Result<Option<RegisteredAgent>, StoreError> {
        let mut agents = self.0.agents.lock().expect("the lock");
        Ok(agents
            .iter_mut()
            .find(|agent| agent.org == org && agent.id == id && !agent.is_revoked())
            .map(|agent| {
                agent.revoked_at_unix_nanos = Some(1_700_000_001_000_000_000);
                agent.clone()
            }))
    }
}

impl ApiKeys for Book {
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

    async fn grant_for(&self, _secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(None)
    }

    async fn agent_standing(
        &self,
        org: TenantId,
        client_id: &str,
    ) -> Result<Option<RegisteredAgent>, StoreError> {
        Ok(self.live(org, client_id))
    }

    async fn agent_used(
        &self,
        org: TenantId,
        agent: AgentId,
        entry: &AuditEntry,
    ) -> Result<(), StoreError> {
        for held in self.agents.lock().expect("the lock").iter_mut() {
            if held.org == org && held.id == agent {
                held.last_used_at_unix_nanos = Some(1_700_000_002_000_000_000);
            }
        }
        self.notebook.record(entry).await
    }
}

#[derive(Default)]
struct Tokens {
    known: Mutex<HashMap<String, Result<VerifiedToken, TokenRefusal>>>,
}

impl Tokens {
    fn mint(&self, name: &str, client_id: &str, scopes: &[&str], jti: &str) -> String {
        let token = format!("head.{name}.sig");
        self.known.lock().expect("the lock").insert(
            token.clone(),
            Ok(VerifiedToken {
                client_id: client_id.into(),
                scopes: scopes.iter().map(|scope| (*scope).into()).collect(),
                jti: jti.into(),
                expires_at_unix_secs: 4_000_000_000,
                delegated: None,
            }),
        );
        token
    }

    fn refuse(&self, name: &str, refusal: TokenRefusal) -> String {
        let token = format!("head.{name}.sig");
        self.known
            .lock()
            .expect("the lock")
            .insert(token.clone(), Err(refusal));
        token
    }
}

impl AgentTokens for Tokens {
    async fn verify(&self, token: &AgentToken) -> Result<VerifiedToken, TokenRefusal> {
        self.known
            .lock()
            .expect("the lock")
            .get(token.exposed())
            .cloned()
            .unwrap_or(Err(TokenRefusal::Invalid))
    }
}

struct Fakes {
    role: OrgRole,
    book: Arc<Book>,
}

impl Untransacted for Fakes {}

impl RepoFor<domain::Audit> for Fakes {
    type Repo = Notebook;

    fn build(&self, (): &()) -> Self::Repo {
        self.book.notebook.clone()
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
    type Repo = Arc<Book>;

    fn build(&self, (): &()) -> Self::Repo {
        Arc::clone(&self.book)
    }
}

impl RepoFor<domain::Agents> for Fakes {
    type Repo = Roll;

    fn build(&self, (): &()) -> Self::Repo {
        Roll(Arc::clone(&self.book))
    }
}

impl RepoFor<domain::Projects> for Fakes {
    type Repo = Storefront;

    fn build(&self, (): &()) -> Self::Repo {
        Storefront
    }
}

struct TestStack;

impl Stack for TestStack {
    type Repos = Fakes;
    type Clock = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Looked;
    type IssueStore = Looked;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Oidc = Inert;
    type Agents = Arc<Tokens>;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

struct Harness {
    book: Arc<Book>,
    tokens: Arc<Tokens>,
    looked: Looked,
    agents_on: bool,
    reason: Option<String>,
}

impl Harness {
    fn new() -> Self {
        Self {
            book: Arc::default(),
            tokens: Arc::default(),
            looked: Looked::default(),
            agents_on: true,
            reason: None,
        }
    }

    fn without_an_issuer() -> Self {
        Self {
            agents_on: false,
            reason: Some("agents.audience is not set".to_owned()),
            ..Self::new()
        }
    }

    fn app(&self, role: OrgRole) -> Router {
        let adapters = Adapters {
            clock: Inert::default(),
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
        let app = Ferristrace::<TestStack>::new(
            Fakes {
                role,
                book: Arc::clone(&self.book),
            },
            adapters,
        )
        .with_agent_tokens(self.agents_on.then(|| Arc::clone(&self.tokens)));
        let state = AppState::new(Arc::new(app), SessionCookie::default())
            .with_agent_policy(self.reason.clone(), Some("ui-client".to_owned()));
        mark_routes(
            agent_router(state.clone())
                .merge(key_router(state.clone()))
                .merge(explore_router(state)),
        )
    }

    async fn served(&self, role: OrgRole, request: Request<Body>) -> (StatusCode, String) {
        let response = self.app(role).oneshot(request).await.expect("a response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, String::from_utf8(body.to_vec()).expect("text"))
    }

    async fn registered(&self, client_id: &str, scopes: &[&str]) -> serde_json::Value {
        let body = serde_json::json!({
            "name": "triage bot",
            "client_id": client_id,
            "scopes": scopes,
        })
        .to_string();
        let (status, body) = self
            .served(
                OrgRole::Admin,
                as_session(Method::POST, &agents_of(MINE), Some(&body)),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        read(&body)
    }

    async fn read_as(&self, token: &str, uri: &str) -> (StatusCode, String) {
        self.served(OrgRole::Member, as_agent(Method::GET, uri, token))
            .await
    }
}

fn agents_of(org: u128) -> String {
    format!("/api/orgs/{}/agents", id(org))
}

fn traces_of(org: u128) -> String {
    format!("/api/orgs/{}/traces?from=1&to=2", id(org))
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

fn as_agent(method: Method, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("a request")
}

fn as_agent_from_the_app(method: Method, uri: &str, token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header("x-requested-with", "fetch")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .expect("a request")
}

fn read(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("json")
}

fn reads_of(book: &Book) -> Vec<AuditEntry> {
    book.notebook
        .entries()
        .into_iter()
        .filter(|entry| entry.action == Action::AgentRead)
        .collect()
}

#[tokio::test]
async fn an_agent_reads_the_traces_of_its_organisation_and_the_read_is_audited_with_its_jti() {
    let harness = Harness::new();
    harness.registered("triage-bot", &["telemetry:read"]).await;
    let token = harness.tokens.mint(
        "one",
        "triage-bot",
        &["telemetry:read", "openid"],
        "jti-for-the-audit",
    );

    let (status, body) = harness.read_as(&token, &traces_of(MINE)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let reads = reads_of(&harness.book);
    assert_eq!(reads.len(), 1);
    assert_eq!(
        reads[0].actor,
        Actor::Agent(harness.book.id_of("triage-bot"))
    );
    assert_eq!(reads[0].detail.get("jti"), Some(Some("jti-for-the-audit")));
    assert_eq!(
        reads[0].detail.get("route"),
        Some(Some("/api/orgs/{org}/traces"))
    );
    let listed = harness
        .served(
            OrgRole::Admin,
            as_session(Method::GET, &agents_of(MINE), None),
        )
        .await;
    assert_eq!(
        read(&listed.1)["agents"][0]["last_used_at_ms"],
        1_700_000_002_000_u64
    );
}

#[tokio::test]
async fn every_read_route_names_itself_in_the_audit_entry() {
    let harness = Harness::new();
    harness.registered("triage-bot", &["telemetry:read"]).await;
    let token = harness
        .tokens
        .mint("one", "triage-bot", &["telemetry:read"], "jti-1");
    let org = id(MINE);

    for route in [
        format!("/api/orgs/{org}/services?from=1&to=2"),
        format!("/api/orgs/{org}/traces/0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a"),
        format!("/api/orgs/{org}/series?from=1&to=2"),
    ] {
        let (status, body) = harness.read_as(&token, &route).await;
        assert_ne!(status, StatusCode::UNAUTHORIZED, "{route}: {body}");
        assert_ne!(status, StatusCode::FORBIDDEN, "{route}: {body}");
    }

    let routes: Vec<_> = reads_of(&harness.book)
        .iter()
        .map(|entry| entry.detail.get("route").flatten().map(str::to_owned))
        .collect();
    assert_eq!(
        routes,
        [
            Some("/api/orgs/{org}/services".to_owned()),
            Some("/api/orgs/{org}/traces/{trace}".to_owned()),
            Some("/api/orgs/{org}/series".to_owned()),
        ]
    );
}

#[tokio::test]
async fn the_same_token_is_refused_by_another_organisation() {
    let harness = Harness::new();
    harness.registered("triage-bot", &["telemetry:read"]).await;
    let token = harness
        .tokens
        .mint("one", "triage-bot", &["telemetry:read"], "jti-1");

    let (status, _) = harness.read_as(&token, &traces_of(THEIRS)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(reads_of(&harness.book).is_empty());
}

#[tokio::test]
async fn an_agent_registered_in_another_organisation_is_refused_here() {
    let harness = Harness::new();
    harness
        .book
        .agents
        .lock()
        .expect("the lock")
        .push(RegisteredAgent {
            id: AgentId::new(Uuid::from_u128(900)),
            org: TenantId::new(Uuid::from_u128(THEIRS)),
            name: "elsewhere".into(),
            client_id: "triage-bot".into(),
            scopes: AgentScopes::new(&[AgentScope::TelemetryRead]).expect("scopes"),
            project: None,
            created_by: None,
            created_at_unix_nanos: 1,
            last_used_at_unix_nanos: None,
            revoked_at_unix_nanos: None,
        });
    let token = harness
        .tokens
        .mint("one", "triage-bot", &["telemetry:read"], "jti-1");

    let (status, _) = harness.read_as(&token, &traces_of(MINE)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(reads_of(&harness.book).is_empty());
}

#[tokio::test]
async fn a_revoked_agent_is_refused_on_its_next_request_whatever_the_token_expiry() {
    let harness = Harness::new();
    let registered = harness.registered("triage-bot", &["telemetry:read"]).await;
    let token = harness
        .tokens
        .mint("one", "triage-bot", &["telemetry:read"], "jti-1");
    let (before, _) = harness.read_as(&token, &traces_of(MINE)).await;
    let revoke = format!(
        "{}/{}",
        agents_of(MINE),
        registered["id"].as_str().expect("an id")
    );

    let (revoked, _) = harness
        .served(OrgRole::Admin, as_session(Method::DELETE, &revoke, None))
        .await;
    let (after, _) = harness.read_as(&token, &traces_of(MINE)).await;

    assert_eq!(before, StatusCode::OK);
    assert_eq!(revoked, StatusCode::NO_CONTENT);
    assert_eq!(after, StatusCode::UNAUTHORIZED);
    assert_eq!(reads_of(&harness.book).len(), 1);
}

#[tokio::test]
async fn a_token_whose_scope_claim_lacks_telemetry_read_is_refused_even_when_granted() {
    let harness = Harness::new();
    harness.registered("triage-bot", &["telemetry:read"]).await;
    let without = harness.tokens.mint(
        "without",
        "triage-bot",
        &["incidents:comment", "openid"],
        "j1",
    );
    let empty = harness.tokens.mint("empty", "triage-bot", &[], "j2");

    for token in [without, empty] {
        let (status, _) = harness.read_as(&token, &traces_of(MINE)).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert!(reads_of(&harness.book).is_empty());
}

#[tokio::test]
async fn an_agent_granted_only_other_scopes_is_refused_even_when_the_token_claims_telemetry_read() {
    let harness = Harness::new();
    harness
        .registered("commenter", &["incidents:comment"])
        .await;
    let token = harness.tokens.mint(
        "greedy",
        "commenter",
        &["telemetry:read", "incidents:comment"],
        "j1",
    );

    let (status, _) = harness.read_as(&token, &traces_of(MINE)).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(reads_of(&harness.book).is_empty());
}

#[tokio::test]
async fn an_unverifiable_token_is_refused_with_401_and_an_unreachable_issuer_with_503() {
    let harness = Harness::new();
    harness.registered("triage-bot", &["telemetry:read"]).await;
    let expired = harness.tokens.refuse("expired", TokenRefusal::Invalid);
    let down = harness.tokens.refuse("down", TokenRefusal::Unavailable);

    let (invalid, _) = harness.read_as(&expired, &traces_of(MINE)).await;
    let (unknown, _) = harness.read_as("head.never.seen", &traces_of(MINE)).await;
    let (unavailable, _) = harness.read_as(&down, &traces_of(MINE)).await;

    assert_eq!(invalid, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown, StatusCode::UNAUTHORIZED);
    assert_eq!(unavailable, StatusCode::SERVICE_UNAVAILABLE);
    assert!(reads_of(&harness.book).is_empty());
}

#[tokio::test]
async fn without_an_issuer_every_agent_token_is_refused_with_401() {
    let harness = Harness::without_an_issuer();
    harness.registered("triage-bot", &["telemetry:read"]).await;
    let token = harness
        .tokens
        .mint("one", "triage-bot", &["telemetry:read"], "jti-1");

    let (read, _) = harness.read_as(&token, &traces_of(MINE)).await;
    let (me, _) = harness
        .read_as(&token, &format!("{}/me", agents_of(MINE)))
        .await;
    let listed = harness
        .served(
            OrgRole::Admin,
            as_session(Method::GET, &agents_of(MINE), None),
        )
        .await;

    assert_eq!(read, StatusCode::UNAUTHORIZED);
    assert_eq!(me, StatusCode::UNAUTHORIZED);
    assert_eq!(listed.0, StatusCode::OK);
    assert_eq!(read_value(&listed.1)["enabled"], false);
    assert_eq!(
        read_value(&listed.1)["reason"],
        "agents.audience is not set"
    );
}

fn read_value(body: &str) -> serde_json::Value {
    read(body)
}

#[tokio::test]
async fn a_project_bound_agent_reads_only_its_project() {
    let harness = Harness::new();
    let body = serde_json::json!({
        "name": "storefront bot",
        "client_id": "storefront-bot",
        "scopes": ["telemetry:read"],
        "project": id(STOREFRONT),
    })
    .to_string();
    let (status, _) = harness
        .served(
            OrgRole::Admin,
            as_session(Method::POST, &agents_of(MINE), Some(&body)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = harness
        .tokens
        .mint("bound", "storefront-bot", &["telemetry:read"], "jti-1");

    let (open, _) = harness.read_as(&token, &traces_of(MINE)).await;
    let (own, _) = harness
        .read_as(
            &token,
            &format!("{}&project={}", traces_of(MINE), id(STOREFRONT)),
        )
        .await;
    let (other, _) = harness
        .read_as(
            &token,
            &format!("{}&project={}", traces_of(MINE), id(ELSEWHERE)),
        )
        .await;

    assert_eq!(open, StatusCode::OK);
    assert_eq!(own, StatusCode::OK);
    assert_eq!(other, StatusCode::NOT_FOUND);
    assert_eq!(
        harness.looked.asked(),
        vec![Some(a_project(STOREFRONT)), Some(a_project(STOREFRONT))]
    );
}

#[tokio::test]
async fn an_agent_never_reaches_session_only_or_admin_routes() {
    let harness = Harness::new();
    let registered = harness
        .registered("triage-bot", &["telemetry:read", "incidents:resolve"])
        .await;
    let token = harness.tokens.mint(
        "one",
        "triage-bot",
        &["telemetry:read", "incidents:resolve"],
        "jti-1",
    );
    let agent = registered["id"].as_str().expect("an id").to_owned();
    let enrol = r#"{"name":"mine","client_id":"mine","scopes":["telemetry:read"]}"#;
    let key = r#"{"name":"mine","scopes":["read"]}"#;
    let routes = [
        (Method::GET, agents_of(MINE), ""),
        (Method::POST, agents_of(MINE), enrol),
        (Method::DELETE, format!("{}/{agent}", agents_of(MINE)), ""),
        (Method::GET, format!("/api/orgs/{}/keys", id(MINE)), ""),
        (Method::POST, format!("/api/orgs/{}/keys", id(MINE)), key),
    ];

    for (method, uri, body) in routes {
        let plain = harness
            .served(
                OrgRole::Admin,
                Request::builder()
                    .method(method.clone())
                    .uri(&uri)
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_owned()))
                    .expect("a request"),
            )
            .await;
        let from_the_app = harness
            .served(
                OrgRole::Admin,
                as_agent_from_the_app(method.clone(), &uri, &token, body),
            )
            .await;

        assert_eq!(plain.0, StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(from_the_app.0, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
    assert_eq!(
        harness
            .book
            .agents
            .lock()
            .expect("the lock")
            .iter()
            .filter(|agent| agent.is_revoked())
            .count(),
        0
    );
    assert_eq!(harness.book.agents.lock().expect("the lock").len(), 1);
}

#[tokio::test]
async fn only_an_agent_token_answers_agents_me_and_it_reports_the_effective_grant() {
    let harness = Harness::new();
    let body = serde_json::json!({
        "name": "storefront bot",
        "client_id": "storefront-bot",
        "scopes": ["telemetry:read", "incidents:comment"],
        "project": id(STOREFRONT),
    })
    .to_string();
    harness
        .served(
            OrgRole::Admin,
            as_session(Method::POST, &agents_of(MINE), Some(&body)),
        )
        .await;
    let token = harness.tokens.mint(
        "bot",
        "storefront-bot",
        &["telemetry:read", "incidents:resolve"],
        "jti-me",
    );
    let me = format!("{}/me", agents_of(MINE));

    let (status, answer) = harness.read_as(&token, &me).await;
    let (as_session_status, _) = harness
        .served(OrgRole::Admin, as_session(Method::GET, &me, None))
        .await;
    let (elsewhere, _) = harness
        .read_as(&token, &format!("{}/me", agents_of(THEIRS)))
        .await;

    assert_eq!(status, StatusCode::OK, "{answer}");
    let grant = read(&answer);
    assert_eq!(grant["client_id"], "storefront-bot");
    assert_eq!(grant["scopes"], serde_json::json!(["telemetry:read"]));
    assert_eq!(grant["project"], id(STOREFRONT));
    assert_eq!(grant["jti"], "jti-me");
    assert_eq!(as_session_status, StatusCode::FORBIDDEN);
    assert_eq!(elsewhere, StatusCode::UNAUTHORIZED);
    let reads = reads_of(&harness.book);
    assert_eq!(reads.len(), 1);
    assert_eq!(
        reads[0].detail.get("route"),
        Some(Some("/api/orgs/{org}/agents/me"))
    );
}

#[tokio::test]
async fn a_token_without_any_effective_scope_cannot_even_ask_for_its_grant() {
    let harness = Harness::new();
    harness.registered("triage-bot", &["telemetry:read"]).await;
    let token = harness.tokens.mint("none", "triage-bot", &["openid"], "j");

    let (status, _) = harness
        .read_as(&token, &format!("{}/me", agents_of(MINE)))
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_administrator_registers_lists_and_revokes_and_each_change_is_audited() {
    let harness = Harness::new();

    let registered = harness
        .registered("triage-bot", &["incidents:comment", "telemetry:read"])
        .await;
    let (listed_status, listed) = harness
        .served(
            OrgRole::Admin,
            as_session(Method::GET, &agents_of(MINE), None),
        )
        .await;
    let revoke = format!(
        "{}/{}",
        agents_of(MINE),
        registered["id"].as_str().expect("an id")
    );
    let (revoked, _) = harness
        .served(OrgRole::Admin, as_session(Method::DELETE, &revoke, None))
        .await;
    let (twice, _) = harness
        .served(OrgRole::Admin, as_session(Method::DELETE, &revoke, None))
        .await;
    let (after, after_body) = harness
        .served(
            OrgRole::Admin,
            as_session(Method::GET, &agents_of(MINE), None),
        )
        .await;

    assert_eq!(registered["client_id"], "triage-bot");
    assert_eq!(
        registered["scopes"],
        serde_json::json!(["telemetry:read", "incidents:comment"])
    );
    assert_eq!(registered["revoked_at_ms"], serde_json::Value::Null);
    assert_eq!(listed_status, StatusCode::OK);
    assert_eq!(read(&listed)["enabled"], true);
    assert_eq!(read(&listed)["agents"].as_array().expect("a list").len(), 1);
    assert_eq!(revoked, StatusCode::NO_CONTENT);
    assert_eq!(twice, StatusCode::NOT_FOUND);
    assert_eq!(after, StatusCode::OK);
    assert_eq!(
        read(&after_body)["agents"][0]["revoked_at_ms"],
        1_700_000_001_000_u64
    );
    let actions: Vec<_> = harness
        .book
        .notebook
        .entries()
        .iter()
        .map(|entry| entry.action)
        .collect();
    assert_eq!(actions, [Action::AgentRegistered, Action::AgentRevoked]);
}

#[tokio::test]
async fn a_member_may_not_register_list_or_revoke_and_a_stranger_finds_no_organisation() {
    let harness = Harness::new();
    let registered = harness.registered("triage-bot", &["telemetry:read"]).await;
    let enrol = r#"{"name":"mine","client_id":"mine","scopes":["telemetry:read"]}"#;
    let revoke = format!(
        "{}/{}",
        agents_of(MINE),
        registered["id"].as_str().expect("an id")
    );

    let (listing, _) = harness
        .served(
            OrgRole::Member,
            as_session(Method::GET, &agents_of(MINE), None),
        )
        .await;
    let (registering, _) = harness
        .served(
            OrgRole::Member,
            as_session(Method::POST, &agents_of(MINE), Some(enrol)),
        )
        .await;
    let (revoking, _) = harness
        .served(OrgRole::Member, as_session(Method::DELETE, &revoke, None))
        .await;
    let (stranger_listing, _) = harness
        .served(
            OrgRole::Owner,
            as_session(Method::GET, &agents_of(THEIRS), None),
        )
        .await;
    let (stranger_registering, _) = harness
        .served(
            OrgRole::Owner,
            as_session(Method::POST, &agents_of(THEIRS), Some(enrol)),
        )
        .await;
    let (no_session, _) = harness
        .served(
            OrgRole::Admin,
            Request::builder()
                .method(Method::GET)
                .uri(agents_of(MINE))
                .header("x-requested-with", "fetch")
                .body(Body::empty())
                .expect("a request"),
        )
        .await;

    assert_eq!(listing, StatusCode::FORBIDDEN);
    assert_eq!(registering, StatusCode::FORBIDDEN);
    assert_eq!(revoking, StatusCode::FORBIDDEN);
    assert_eq!(stranger_listing, StatusCode::NOT_FOUND);
    assert_eq!(stranger_registering, StatusCode::NOT_FOUND);
    assert_eq!(no_session, StatusCode::UNAUTHORIZED);
    assert_eq!(harness.book.agents.lock().expect("the lock").len(), 1);
}

#[tokio::test]
async fn a_registration_is_validated() {
    let harness = Harness::new();
    harness.registered("taken", &["telemetry:read"]).await;
    let cases = [
        (
            r#"{"name":"","client_id":"a","scopes":["telemetry:read"]}"#.to_owned(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            r#"{"name":"a","client_id":"  ","scopes":["telemetry:read"]}"#.to_owned(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            r#"{"name":"a","client_id":"two words","scopes":["telemetry:read"]}"#.to_owned(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            r#"{"name":"a","client_id":"a","scopes":[]}"#.to_owned(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            r#"{"name":"a","client_id":"a","scopes":["admin"]}"#.to_owned(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            r#"{"name":"a","client_id":"a","scopes":["read"]}"#.to_owned(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            r#"{"name":"a","client_id":"a","scopes":["telemetry:read","telemetry:read"]}"#
                .to_owned(),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            r#"{"name":"a","client_id":"taken","scopes":["telemetry:read"]}"#.to_owned(),
            StatusCode::CONFLICT,
        ),
        (
            format!(
                r#"{{"name":"a","client_id":"b","scopes":["telemetry:read"],"project":"{}"}}"#,
                id(ELSEWHERE)
            ),
            StatusCode::NOT_FOUND,
        ),
    ];

    for (body, expected) in cases {
        let (status, answer) = harness
            .served(
                OrgRole::Admin,
                as_session(Method::POST, &agents_of(MINE), Some(&body)),
            )
            .await;

        assert_eq!(status, expected, "{body}: {answer}");
    }
    assert_eq!(harness.book.agents.lock().expect("the lock").len(), 1);
}

#[tokio::test]
async fn the_client_people_sign_in_with_cannot_be_registered_as_an_agent() {
    let harness = Harness::new();
    let body = r#"{"name":"ui","client_id":"ui-client","scopes":["telemetry:read"]}"#;

    let (status, _) = harness
        .served(
            OrgRole::Admin,
            as_session(Method::POST, &agents_of(MINE), Some(body)),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(harness.book.agents.lock().expect("the lock").is_empty());
}
