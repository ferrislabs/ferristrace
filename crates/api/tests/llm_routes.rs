use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, llm_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Notebook, PriceLedger, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Action, ApiKey, ApiKeys, ApiSecret, BucketTotals, Call, CallCursor, CallPage,
    CallQuery, Counted, Deploy, DeployFeed, Deployed, Fingerprint, GroupBy, GroupLatency,
    IssueFilter, IssueReader, IssuedKey, KeyId, KeyName, KeyScopes, KeyTtl, LlmReader, Membership,
    Memberships, NoPhotos, Occurrence, OrgRole, PricedCall, ProjectId, PromptMarker, ReadGrant,
    Release, ReleaseDetail, ReleaseFilter, ReleaseReader, SeenModel, Session, SessionError,
    SessionStore, SessionToken, SpanId, SpendQuery, SpendRead, StoreError, TargetKind, TenantId,
    TraceId, UnixSeconds, WhoamiError, Window,
};
use ferristrace_infra::FixedClock;
use serde_json::{Value, json};
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

struct MemberOfMine(OrgRole);

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: org_id(MINE),
            name: "Acme".into(),
            role: self.0,
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

struct Fakes {
    role: OrgRole,
    notebook: Notebook,
    prices: PriceLedger,
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
        MemberOfMine(self.role)
    }
}

impl RepoFor<domain::ApiKeys> for Fakes {
    type Repo = TwoReadKeys;

    fn build(&self, (): &()) -> Self::Repo {
        TwoReadKeys
    }
}

impl RepoFor<domain::Audit> for Fakes {
    type Repo = Notebook;

    fn build(&self, (): &()) -> Self::Repo {
        self.notebook.clone()
    }
}

impl RepoFor<domain::ModelPrices> for Fakes {
    type Repo = PriceLedger;

    fn build(&self, (): &()) -> Self::Repo {
        self.prices.clone()
    }
}

type Asked = (TenantId, Option<ProjectId>);

#[derive(Debug, Default)]
struct Shelf {
    asked: Mutex<Vec<Asked>>,
    queries: Mutex<Vec<CallQuery>>,
    spends: Mutex<Vec<SpendQuery>>,
    broken: bool,
}

impl Shelf {
    fn asked(&self) -> Vec<Asked> {
        self.asked.lock().expect("the lock").clone()
    }

    fn spends(&self) -> Vec<SpendQuery> {
        self.spends.lock().expect("the lock").clone()
    }

    fn queries(&self) -> Vec<CallQuery> {
        self.queries.lock().expect("the lock").clone()
    }
}

struct Store(Arc<Shelf>);

const CALL_AT_NANOS: u64 = 1_700_000_100_123_456_789;

fn a_call(seed: u8, model: &str, at_nanos: u64) -> Call {
    Call {
        time_unix_nanos: at_nanos,
        trace: TraceId::try_from([seed; 16].as_slice()).expect("a trace"),
        span: SpanId::try_from([seed; 8].as_slice()).expect("a span"),
        service: "assistant".into(),
        provider: "openai".into(),
        operation: "chat".into(),
        request_model: model.into(),
        response_model: String::new().into(),
        input_tokens: 1_000_000,
        output_tokens: 500_000,
        cached_input_tokens: 200_000,
        duration_nanos: 2_500_000_000,
        error: true,
        feature: "ticket-summary".into(),
        customer: "acme".into(),
        user: "user-7".into(),
        prompt_version: "v3".into(),
        environment: "prod".into(),
    }
}

impl LlmReader for Store {
    async fn calls(&self, org: TenantId, query: &CallQuery) -> Result<CallPage, StoreError> {
        self.0
            .asked
            .lock()
            .expect("the lock")
            .push((org, query.project));
        self.0.queries.lock().expect("the lock").push(query.clone());
        if self.0.broken {
            return Err(StoreError::Unreachable);
        }
        Ok(CallPage {
            calls: vec![
                PricedCall {
                    call: a_call(1, "gpt-4o", CALL_AT_NANOS),
                    cost: None,
                },
                PricedCall {
                    call: a_call(2, "mystery-1", CALL_AT_NANOS),
                    cost: None,
                },
            ],
            next: Some(CallCursor {
                time_unix_nanos: CALL_AT_NANOS,
                tiebreak: 77,
            }),
        })
    }

    async fn spend(&self, org: TenantId, query: &SpendQuery) -> Result<SpendRead, StoreError> {
        self.0
            .asked
            .lock()
            .expect("the lock")
            .push((org, query.filter.project));
        self.0.spends.lock().expect("the lock").push(query.clone());
        if self.0.broken {
            return Err(StoreError::Unreachable);
        }
        let bucket = |key: &str, model: &str, slot: u64| BucketTotals {
            key: key.into(),
            provider: "openai".into(),
            request_model: model.into(),
            response_model: String::new().into(),
            slot,
            calls: 2,
            errors: 1,
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cached_input_tokens: 0,
        };
        Ok(SpendRead {
            buckets: vec![
                bucket("ticket-summary", "gpt-4o", 3),
                bucket("ticket-summary", "mystery-1", 4),
                bucket("search", "gpt-4o", 3),
            ],
            latencies: vec![GroupLatency {
                key: "ticket-summary".into(),
                p50_nanos: 1_500_000_000,
                p95_nanos: 2_500_000_000,
            }],
            markers: vec![PromptMarker {
                time_unix_nanos: CALL_AT_NANOS,
                feature: "ticket-summary".into(),
                prompt_version: "v3".into(),
            }],
        })
    }

    async fn models(
        &self,
        org: TenantId,
        _window: Window,
        project: Option<ProjectId>,
    ) -> Result<Vec<SeenModel>, StoreError> {
        self.0.asked.lock().expect("the lock").push((org, project));
        Ok(vec![
            SeenModel {
                provider: "openai".into(),
                request_model: "gpt-4o".into(),
                response_model: "gpt-4o-2024-08-06".into(),
                calls: 5,
                last_unix_nanos: CALL_AT_NANOS,
            },
            SeenModel {
                provider: "openai".into(),
                request_model: "mystery-1".into(),
                response_model: String::new().into(),
                calls: 3,
                last_unix_nanos: CALL_AT_NANOS,
            },
        ])
    }
}

impl IssueReader for Store {
    async fn issues(
        &self,
        _tenant: TenantId,
        _filter: &IssueFilter,
    ) -> Result<Vec<Counted>, StoreError> {
        Ok(Vec::new())
    }

    async fn occurrences(
        &self,
        _tenant: TenantId,
        _project: ProjectId,
        _print: Fingerprint,
        _window: Window,
        _limit: u32,
    ) -> Result<Vec<Occurrence>, StoreError> {
        Ok(Vec::new())
    }
}

impl DeployFeed for Store {
    async fn deployed(&self, _window: Window) -> Result<Vec<Deployed>, StoreError> {
        Ok(Vec::new())
    }
}

impl ReleaseReader for Store {
    async fn releases(
        &self,
        _org: TenantId,
        _filter: &ReleaseFilter,
    ) -> Result<Vec<Release>, StoreError> {
        Ok(Vec::new())
    }

    async fn release(
        &self,
        _org: TenantId,
        _service: &str,
        _version: &str,
        _project: Option<ProjectId>,
    ) -> Result<Option<ReleaseDetail>, StoreError> {
        Ok(None)
    }

    async fn deploys(
        &self,
        _org: TenantId,
        _filter: &ReleaseFilter,
    ) -> Result<Vec<Deploy>, StoreError> {
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
    type IssueStore = Store;
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

struct Harness {
    shelf: Arc<Shelf>,
    notebook: Notebook,
    prices: PriceLedger,
}

impl Harness {
    fn new() -> Self {
        Self::over(Shelf::default())
    }

    fn over(shelf: Shelf) -> Self {
        Self {
            shelf: Arc::new(shelf),
            notebook: Notebook::default(),
            prices: PriceLedger::default(),
        }
    }

    fn app(&self, role: OrgRole, with_store: bool) -> Router {
        let adapters = Adapters {
            clock: FixedClock::at(NOW),
            mailer: Inert::default(),
            webhooks: Inert::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: None,
            issue_store: with_store.then(|| Store(self.shelf.clone())),
            log_store: None,
            metric_store: None,
            users: Some(Bearers),
        };
        llm_router(AppState::new(Arc::new(Ferristrace::<TestStack>::new(
            Fakes {
                role,
                notebook: self.notebook.clone(),
                prices: self.prices.clone(),
            },
            adapters,
        ))))
    }

    async fn send(
        &self,
        role: OrgRole,
        request: Request<Body>,
        with_store: bool,
    ) -> (StatusCode, Value) {
        let response = self
            .app(role, with_store)
            .oneshot(request)
            .await
            .expect("a response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let value = serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
        (status, value)
    }

    async fn session(
        &self,
        role: OrgRole,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.send(role, as_session(method, uri, body), true).await
    }

    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.session(OrgRole::Admin, method, uri, body).await
    }

    async fn key(&self, key: &str, uri: &str) -> (StatusCode, Value) {
        self.send(OrgRole::Member, as_key(key, uri), true).await
    }
}

fn as_session(method: Method, uri: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"));
    match body {
        Some(body) => builder
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("a request")
}

fn as_key(key: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {key}"))
        .body(Body::empty())
        .expect("a request")
}

fn org_id(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn project_id(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn calls_of(seed: u128, rest: &str) -> String {
    format!(
        "/api/orgs/{}/llm/calls?from=1700000000000&to=1700001000000{rest}",
        Uuid::from_u128(seed)
    )
}

fn spend_of(seed: u128, rest: &str) -> String {
    format!(
        "/api/orgs/{}/llm/spend?from=1700000000000&to=1700000600000{rest}",
        Uuid::from_u128(seed)
    )
}

fn unpriced_of(seed: u128) -> String {
    format!(
        "/api/orgs/{}/llm/unpriced?from=1700000000000&to=1700001000000",
        Uuid::from_u128(seed)
    )
}

fn prices_of(seed: u128) -> String {
    format!("/api/orgs/{}/llm/prices", Uuid::from_u128(seed))
}

fn a_price(model: &str, effective_from_ms: u64) -> Value {
    json!({
        "provider": "OpenAI",
        "model": model,
        "input": 2_000_000,
        "output": 8_000_000,
        "cached_input": 500_000,
        "effective_from_ms": effective_from_ms,
    })
}

#[tokio::test]
async fn spec_l4_1_calls_come_in_the_agreed_shape_with_a_null_cost_when_unpriced() {
    let harness = Harness::new();

    let (status, body) = harness
        .session(OrgRole::Member, Method::GET, &calls_of(MINE, ""), None)
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let first = &body["calls"][0];
    assert_eq!(first["time_ms"], 1_700_000_100_123_u64);
    assert_eq!(first["trace_id"], "01".repeat(16));
    assert_eq!(first["span_id"], "01".repeat(8));
    assert_eq!(first["service"], "assistant");
    assert_eq!(first["provider"], "openai");
    assert_eq!(first["operation"], "chat");
    assert_eq!(first["model"], "gpt-4o");
    assert_eq!(first["input_tokens"], 1_000_000);
    assert_eq!(first["output_tokens"], 500_000);
    assert_eq!(first["cached_input_tokens"], 200_000);
    assert_eq!(first["cost"], Value::Null);
    assert_eq!(first["duration_ms"], 2500.0);
    assert_eq!(first["error"], true);
    assert_eq!(first["feature"], "ticket-summary");
    assert_eq!(first["customer"], "acme");
    assert_eq!(first["user"], "user-7");
    assert_eq!(first["prompt_version"], "v3");
    assert_eq!(first["environment"], "prod");
    assert_eq!(body["calls"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["next"], format!("{CALL_AT_NANOS}.77"));
}

#[tokio::test]
async fn spec_l4_2_a_price_sets_the_cost_of_the_calls_of_its_model_and_leaves_others_null() {
    let harness = Harness::new();
    let (created, _) = harness
        .admin(Method::POST, &prices_of(MINE), Some(a_price("gpt-4o*", 0)))
        .await;

    let (status, body) = harness
        .session(OrgRole::Member, Method::GET, &calls_of(MINE, ""), None)
        .await;

    assert_eq!(created, StatusCode::CREATED);
    assert_eq!(status, StatusCode::OK, "{body}");
    let cost = body["calls"][0]["cost"].as_f64().expect("a cost");
    let expected = 800_000.0 * 2.0 + 200_000.0 * 0.5 + 500_000.0 * 8.0;
    assert!((cost - expected / 1_000_000.0).abs() < 1e-9, "{cost}");
    assert_eq!(body["calls"][1]["cost"], Value::Null);
}

#[tokio::test]
async fn spec_l4_2_a_price_changes_the_cost_only_from_its_effective_date() {
    let harness = Harness::new();
    let after_the_call = 1_700_000_100_124_u64;
    harness
        .admin(
            Method::POST,
            &prices_of(MINE),
            Some(a_price("gpt-4o", after_the_call)),
        )
        .await;

    let (_, before) = harness
        .session(OrgRole::Member, Method::GET, &calls_of(MINE, ""), None)
        .await;
    harness
        .admin(
            Method::POST,
            &prices_of(MINE),
            Some(a_price("gpt-4o", 1_700_000_100_123)),
        )
        .await;
    let (_, at) = harness
        .session(OrgRole::Member, Method::GET, &calls_of(MINE, ""), None)
        .await;

    assert_eq!(before["calls"][0]["cost"], Value::Null);
    assert!(at["calls"][0]["cost"].as_f64().is_some());
}

#[tokio::test]
async fn spec_l4_2_the_unpriced_models_are_listed_with_their_call_counts() {
    let harness = Harness::new();
    harness
        .admin(Method::POST, &prices_of(MINE), Some(a_price("gpt-4o", 0)))
        .await;

    let (status, body) = harness
        .session(OrgRole::Member, Method::GET, &unpriced_of(MINE), None)
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({"models": [{"provider": "openai", "model": "mystery-1", "calls": 3, "last_seen_ms": 1_700_000_100_123_u64}]})
    );
}

#[tokio::test]
async fn spec_l4_2_prices_are_created_listed_and_deleted_by_an_admin_and_audited() {
    let harness = Harness::new();

    let (created_status, created) = harness
        .admin(Method::POST, &prices_of(MINE), Some(a_price("gpt-4o", 0)))
        .await;
    let (listed_status, listed) = harness
        .session(OrgRole::Member, Method::GET, &prices_of(MINE), None)
        .await;
    let id = created["id"].as_str().expect("an id").to_owned();
    let (deleted, _) = harness
        .admin(Method::DELETE, &format!("{}/{id}", prices_of(MINE)), None)
        .await;
    let (again, _) = harness
        .admin(Method::DELETE, &format!("{}/{id}", prices_of(MINE)), None)
        .await;
    let (_, after) = harness
        .session(OrgRole::Member, Method::GET, &prices_of(MINE), None)
        .await;

    assert_eq!(created_status, StatusCode::CREATED, "{created}");
    assert_eq!(created["provider"], "openai");
    assert_eq!(created["model"], "gpt-4o");
    assert_eq!(created["input"], 2_000_000);
    assert_eq!(created["output"], 8_000_000);
    assert_eq!(created["cached_input"], 500_000);
    assert_eq!(created["effective_from_ms"], 0);
    assert_eq!(listed_status, StatusCode::OK);
    assert_eq!(listed["prices"], json!([created]));
    assert_eq!(deleted, StatusCode::NO_CONTENT);
    assert_eq!(again, StatusCode::NOT_FOUND);
    assert_eq!(after["prices"], json!([]));
    let entries = harness.notebook.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].action, Action::PriceCreated);
    assert_eq!(entries[0].target.kind, TargetKind::Price);
    assert_eq!(entries[0].target.id.as_deref(), Some(id.as_str()));
    assert_eq!(entries[0].org, org_id(MINE));
    assert_eq!(entries[1].action, Action::PriceDeleted);
}

#[tokio::test]
async fn spec_l4_2_a_member_reads_prices_but_cannot_write_them() {
    let harness = Harness::new();

    let (created, _) = harness
        .session(
            OrgRole::Member,
            Method::POST,
            &prices_of(MINE),
            Some(a_price("gpt-4o", 0)),
        )
        .await;
    let (deleted, _) = harness
        .session(
            OrgRole::Member,
            Method::DELETE,
            &format!("{}/{}", prices_of(MINE), Uuid::from_u128(5)),
            None,
        )
        .await;
    let (listed, _) = harness
        .session(OrgRole::Member, Method::GET, &prices_of(MINE), None)
        .await;

    assert_eq!(created, StatusCode::FORBIDDEN);
    assert_eq!(deleted, StatusCode::FORBIDDEN);
    assert_eq!(listed, StatusCode::OK);
    assert!(harness.notebook.entries().is_empty());
}

#[tokio::test]
async fn spec_l4_2_a_read_key_reads_prices_calls_and_unpriced_but_cannot_write() {
    let harness = Harness::new();
    let write = Request::builder()
        .method(Method::POST)
        .uri(prices_of(MINE))
        .header(AUTHORIZATION, format!("Bearer {ORG_KEY}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(a_price("gpt-4o", 0).to_string()))
        .expect("a request");

    let (prices, _) = harness.key(ORG_KEY, &prices_of(MINE)).await;
    let (calls, _) = harness.key(PROJECT_KEY, &calls_of(MINE, "")).await;
    let (unpriced, _) = harness.key(ORG_KEY, &unpriced_of(MINE)).await;
    let (written, _) = harness.send(OrgRole::Member, write, true).await;

    assert_eq!(prices, StatusCode::OK);
    assert_eq!(calls, StatusCode::OK);
    assert_eq!(unpriced, StatusCode::OK);
    assert_eq!(written, StatusCode::UNAUTHORIZED);
    assert!(harness.prices.prices(org_id(MINE)).is_empty());
}

#[tokio::test]
async fn spec_l4_2_a_duplicate_a_malformed_and_an_unbounded_price_are_refused() {
    let harness = Harness::new();
    harness
        .admin(Method::POST, &prices_of(MINE), Some(a_price("gpt-4o", 7)))
        .await;

    let (duplicate, _) = harness
        .admin(Method::POST, &prices_of(MINE), Some(a_price("gpt-4o", 7)))
        .await;
    let (starred, _) = harness
        .admin(Method::POST, &prices_of(MINE), Some(a_price("gp*t", 7)))
        .await;
    let (unbounded, _) = harness
        .admin(
            Method::POST,
            &prices_of(MINE),
            Some(json!({"provider": "openai", "model": "x", "input": 99_999_999_999_999_u64, "output": 1})),
        )
        .await;
    let (missing, _) = harness
        .admin(
            Method::POST,
            &prices_of(MINE),
            Some(json!({"provider": "openai"})),
        )
        .await;

    assert_eq!(duplicate, StatusCode::CONFLICT);
    assert_eq!(starred, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(unbounded, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(missing.is_client_error());
    assert_eq!(harness.notebook.entries().len(), 1);
}

#[tokio::test]
async fn spec_l4_1_the_filters_reach_the_reader() {
    let harness = Harness::new();
    let trace = "ab".repeat(16);
    let rest = format!(
        "&feature=ticket-summary&model=gpt-4o&customer=acme&user=user-7&prompt=v3&provider=openai&environment=prod&trace={trace}&project={}&limit=25&before=5.9",
        Uuid::from_u128(OURS)
    );

    let (status, body) = harness
        .session(OrgRole::Member, Method::GET, &calls_of(MINE, &rest), None)
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let query = harness.shelf.queries().remove(0);
    assert_eq!(query.feature.as_deref(), Some("ticket-summary"));
    assert_eq!(query.model.as_deref(), Some("gpt-4o"));
    assert_eq!(query.customer.as_deref(), Some("acme"));
    assert_eq!(query.user.as_deref(), Some("user-7"));
    assert_eq!(query.prompt_version.as_deref(), Some("v3"));
    assert_eq!(query.provider.as_deref(), Some("openai"));
    assert_eq!(query.environment.as_deref(), Some("prod"));
    assert_eq!(
        query.trace,
        Some(TraceId::try_from([0xab; 16].as_slice()).expect("a trace"))
    );
    assert_eq!(query.project, Some(project_id(OURS)));
    assert_eq!(query.limit(), 25);
    assert_eq!(
        query.before,
        Some(CallCursor {
            time_unix_nanos: 5,
            tiebreak: 9
        })
    );
    assert_eq!(query.window.from_unix_nanos(), 1_700_000_000_000_000_000);
}

#[tokio::test]
async fn spec_l4_1_a_malformed_request_is_refused_before_the_reader() {
    let harness = Harness::new();
    let cases = [
        calls_of(MINE, "&trace=nothex"),
        calls_of(MINE, "&trace=abcd"),
        calls_of(MINE, "&before=nodot"),
        calls_of(MINE, "&limit=0"),
        calls_of(MINE, "&limit=100000"),
        calls_of(MINE, "&project=nope"),
        format!("/api/orgs/{}/llm/calls?from=5&to=5", Uuid::from_u128(MINE)),
        format!("/api/orgs/{}/llm/calls", Uuid::from_u128(MINE)),
    ];

    for uri in cases {
        let (status, _) = harness
            .session(OrgRole::Member, Method::GET, &uri, None)
            .await;
        assert!(status.is_client_error(), "{uri} gave {status}");
    }
    assert!(harness.shelf.asked().is_empty());
}

#[tokio::test]
async fn spec_l4_1_a_project_key_is_narrowed_and_cannot_widen() {
    let harness = Harness::new();

    let (narrowed, _) = harness.key(PROJECT_KEY, &calls_of(MINE, "")).await;
    let (own, _) = harness
        .key(
            PROJECT_KEY,
            &calls_of(MINE, &format!("&project={}", Uuid::from_u128(OURS))),
        )
        .await;
    let (widened, _) = harness
        .key(
            PROJECT_KEY,
            &calls_of(MINE, &format!("&project={}", Uuid::from_u128(OTHER))),
        )
        .await;
    let (models, _) = harness.key(PROJECT_KEY, &unpriced_of(MINE)).await;

    assert_eq!(narrowed, StatusCode::OK);
    assert_eq!(own, StatusCode::OK);
    assert_eq!(widened, StatusCode::NOT_FOUND);
    assert_eq!(models, StatusCode::OK);
    assert_eq!(
        harness.shelf.asked(),
        vec![
            (org_id(MINE), Some(project_id(OURS))),
            (org_id(MINE), Some(project_id(OURS))),
            (org_id(MINE), Some(project_id(OURS))),
        ]
    );
}

#[tokio::test]
async fn spec_l4_1_nobody_reads_another_organisations_calls_or_prices() {
    let harness = Harness::new();

    let (calls, _) = harness
        .session(OrgRole::Admin, Method::GET, &calls_of(THEIRS, ""), None)
        .await;
    let (unpriced, _) = harness
        .session(OrgRole::Admin, Method::GET, &unpriced_of(THEIRS), None)
        .await;
    let (prices, _) = harness
        .session(OrgRole::Admin, Method::GET, &prices_of(THEIRS), None)
        .await;
    let (written, _) = harness
        .admin(Method::POST, &prices_of(THEIRS), Some(a_price("gpt-4o", 0)))
        .await;
    let (by_key, _) = harness.key(ORG_KEY, &calls_of(THEIRS, "")).await;

    assert_eq!(calls, StatusCode::NOT_FOUND);
    assert_eq!(unpriced, StatusCode::NOT_FOUND);
    assert_eq!(prices, StatusCode::NOT_FOUND);
    assert_eq!(written, StatusCode::NOT_FOUND);
    assert_eq!(by_key, StatusCode::NOT_FOUND);
    assert!(harness.shelf.asked().is_empty());
    assert!(harness.prices.prices(org_id(THEIRS)).is_empty());
}

#[tokio::test]
async fn spec_l4_1_an_unauthenticated_request_and_a_dead_store_are_refused() {
    let harness = Harness::new();
    let anonymous = Request::builder()
        .uri(calls_of(MINE, ""))
        .body(Body::empty())
        .expect("a request");
    let broken = Harness::over(Shelf {
        broken: true,
        ..Shelf::default()
    });

    let (unauthorised, _) = harness.send(OrgRole::Member, anonymous, true).await;
    let (unavailable, _) = broken
        .session(OrgRole::Member, Method::GET, &calls_of(MINE, ""), None)
        .await;
    let (unconfigured, _) = harness
        .send(
            OrgRole::Member,
            as_session(Method::GET, &calls_of(MINE, ""), None),
            false,
        )
        .await;

    assert_eq!(unauthorised, StatusCode::UNAUTHORIZED);
    assert_eq!(unavailable, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(unconfigured, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn spec_l4_3_spend_comes_in_the_agreed_shape_priced_per_group() {
    let harness = Harness::new();
    harness
        .admin(Method::POST, &prices_of(MINE), Some(a_price("gpt-4o", 0)))
        .await;

    let (status, body) = harness
        .session(OrgRole::Member, Method::GET, &spend_of(MINE, ""), None)
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["step_ms"], 10_000);
    let groups = body["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 2);
    let first = &groups[0];
    assert_eq!(first["key"], "ticket-summary");
    let each = 2.0 + 0.5 * 8.0;
    assert!((first["cost"].as_f64().expect("a cost") - each).abs() < 1e-9);
    assert_eq!(first["unpriced"], 2);
    assert_eq!(first["calls"], 4);
    assert_eq!(first["input_tokens"], 2_000_000);
    assert_eq!(first["output_tokens"], 1_000_000);
    assert_eq!(first["p50_ms"], 1500.0);
    assert_eq!(first["p95_ms"], 2500.0);
    assert_eq!(first["errors"], 2);
    let points = first["points"].as_array().expect("points");
    assert_eq!(points.len(), 60);
    assert_eq!(points[3][0], 1_700_000_000_000_u64 + 3 * 10_000);
    assert!((points[3][1].as_f64().expect("a cost") - each).abs() < 1e-9);
    assert_eq!(points[4][1], 0.0);
    assert_eq!(groups[1]["key"], "search");
    assert_eq!(groups[1]["p95_ms"], 0.0);
    assert_eq!(body["markers"][0]["time_ms"], 1_700_000_100_123_u64);
    assert_eq!(body["markers"][0]["feature"], "ticket-summary");
    assert_eq!(body["markers"][0]["prompt_version"], "v3");
}

#[tokio::test]
async fn spec_l4_3_group_by_and_filters_reach_the_reader_and_default_to_feature() {
    let harness = Harness::new();
    let rest = format!(
        "&group_by=customer&feature=f&model=m&customer=c&user=u&prompt=v3&project={}",
        Uuid::from_u128(OURS)
    );

    let (status, _) = harness
        .session(OrgRole::Member, Method::GET, &spend_of(MINE, &rest), None)
        .await;
    harness
        .session(OrgRole::Member, Method::GET, &spend_of(MINE, ""), None)
        .await;

    assert_eq!(status, StatusCode::OK);
    let queries = harness.shelf.spends();
    assert_eq!(queries[0].group_by, GroupBy::Customer);
    assert_eq!(queries[0].filter.feature.as_deref(), Some("f"));
    assert_eq!(queries[0].filter.model.as_deref(), Some("m"));
    assert_eq!(queries[0].filter.customer.as_deref(), Some("c"));
    assert_eq!(queries[0].filter.user.as_deref(), Some("u"));
    assert_eq!(queries[0].filter.prompt_version.as_deref(), Some("v3"));
    assert_eq!(queries[0].filter.project, Some(project_id(OURS)));
    assert_eq!(queries[1].group_by, GroupBy::Feature);
}

#[tokio::test]
async fn spec_l4_3_an_unknown_group_by_or_window_is_a_bad_request() {
    let harness = Harness::new();

    let (user, _) = harness
        .session(
            OrgRole::Member,
            Method::GET,
            &spend_of(MINE, "&group_by=user"),
            None,
        )
        .await;
    let (backwards, _) = harness
        .session(
            OrgRole::Member,
            Method::GET,
            &format!("/api/orgs/{}/llm/spend?from=5&to=5", Uuid::from_u128(MINE)),
            None,
        )
        .await;

    assert_eq!(user, StatusCode::BAD_REQUEST);
    assert_eq!(backwards, StatusCode::BAD_REQUEST);
    assert!(harness.shelf.asked().is_empty());
}

#[tokio::test]
async fn spec_l4_3_a_project_key_is_narrowed_and_other_organisations_are_hidden() {
    let harness = Harness::new();

    let (narrowed, _) = harness.key(PROJECT_KEY, &spend_of(MINE, "")).await;
    let (widened, _) = harness
        .key(
            PROJECT_KEY,
            &spend_of(MINE, &format!("&project={}", Uuid::from_u128(OTHER))),
        )
        .await;
    let (theirs, _) = harness.key(ORG_KEY, &spend_of(THEIRS, "")).await;
    let (session, _) = harness
        .session(OrgRole::Admin, Method::GET, &spend_of(THEIRS, ""), None)
        .await;
    let (dead, _) = Harness::over(Shelf {
        broken: true,
        ..Shelf::default()
    })
    .session(OrgRole::Member, Method::GET, &spend_of(MINE, ""), None)
    .await;

    assert_eq!(narrowed, StatusCode::OK);
    assert_eq!(widened, StatusCode::NOT_FOUND);
    assert_eq!(theirs, StatusCode::NOT_FOUND);
    assert_eq!(session, StatusCode::NOT_FOUND);
    assert_eq!(dead, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        harness.shelf.asked(),
        vec![(org_id(MINE), Some(project_id(OURS)))]
    );
}
