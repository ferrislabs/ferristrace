use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, alert_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{
    AlertLedger, Couriered, IncidentLedger, Inert, Notebook, OncallLedger, PriceLedger,
    StatusLedger, Untransacted,
};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Action, Agent, AlertEvent, AlertStatus, ApiKey, ApiKeys, ApiSecret, Assigned,
    Created, EventId, IssuedKey, KeyId, KeyName, KeyScopes, Labels, LiveSession, Membership,
    Memberships, NoPhotos, OrgRole, Project, ProjectId, ProjectName, Projects, ProvisionPolicy,
    ReadGrant, Renamed, RuleId, ServiceFilter, ServiceSeries, ServiceSummary, Session,
    SessionError, SessionId, SessionRoll, SessionStore, SessionToken, Span, SpanCursor, SpanPage,
    SpanPages, SpanReader, StoreError, TargetKind, TeamId, TenantId, TraceFilter, TraceId,
    TraceSummary, UnixSeconds, WhoamiError,
};
use ferristrace_infra::FixedClock;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const OURS: u128 = 30;
const ME: u128 = 9;
const NOW: u64 = 1_700_000_000;
const BOUND_KEY: &str = "ft_live_bound.secret";
const WHOLE_KEY: &str = "ft_live_whole.secret";

fn id(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

#[derive(Clone)]
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
                account: AccountId::new(Uuid::from_u128(ME)),
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

struct TwoKeys;

impl ApiKeys for TwoKeys {
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
        let org = TenantId::new(Uuid::from_u128(MINE));
        Ok(match secret.exposed() {
            BOUND_KEY => Some(ReadGrant {
                org,
                project: Some(ProjectId::new(Uuid::from_u128(OURS))),
            }),
            WHOLE_KEY => Some(ReadGrant::whole(org)),
            _ => None,
        })
    }
}

struct OneProject;

impl Projects for OneProject {
    async fn of(&self, tenant: TenantId) -> Result<Vec<Project>, StoreError> {
        Ok(vec![Project {
            id: ProjectId::new(Uuid::from_u128(OURS)),
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
        Ok(Assigned::Gone)
    }
}

struct Steady;

impl ferristrace_domain::PlacementReader for Steady {
    async fn placements(
        &self,
        _tenant: TenantId,
        _window: ferristrace_domain::Window,
        _project: Option<ProjectId>,
    ) -> Result<ferristrace_domain::Placements, StoreError> {
        Ok(ferristrace_domain::Placements::default())
    }
}

impl ferristrace_domain::CulpritReader for Steady {
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

impl SpanReader for Steady {
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
        filter: &ServiceFilter,
    ) -> Result<ServiceSeries, StoreError> {
        Ok(ServiceSeries {
            step_nanos: filter.step_nanos(),
            requests: 100,
            errors: 10,
            p50_nanos: Some(1_000_000),
            p95_nanos: Some(5_000_000),
            p99_nanos: Some(9_000_000),
            points: Vec::new(),
        })
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

impl SpanPages for Steady {
    async fn page(
        &self,
        _org: TenantId,
        _after: Option<&SpanCursor>,
        _limit: u32,
    ) -> Result<SpanPage, StoreError> {
        Ok(SpanPage::default())
    }
}

struct Fakes {
    role: OrgRole,
    ledger: AlertLedger,
    notebook: Notebook,
    oncall: OncallLedger,
}

impl Untransacted for Fakes {}

macro_rules! repo {
    ($marker:ident, $repo:ty, |$fakes:ident| $built:expr) => {
        impl RepoFor<domain::$marker> for Fakes {
            type Repo = $repo;

            fn build(&self, (): &()) -> Self::Repo {
                let $fakes = self;
                $built
            }
        }
    };
}

repo!(Sessions, OneSession, |fakes| {
    let _ = fakes;
    OneSession
});
repo!(Memberships, MemberOfMine, |fakes| MemberOfMine(fakes.role));
repo!(ApiKeys, TwoKeys, |fakes| {
    let _ = fakes;
    TwoKeys
});
repo!(Projects, OneProject, |fakes| {
    let _ = fakes;
    OneProject
});
repo!(Audit, Notebook, |fakes| fakes.notebook.clone());
repo!(AlertRules, AlertLedger, |fakes| fakes.ledger.clone());
repo!(AlertStates, AlertLedger, |fakes| fakes.ledger.clone());
repo!(AlertEvents, AlertLedger, |fakes| fakes.ledger.clone());
repo!(AlertOutbox, AlertLedger, |fakes| fakes.ledger.clone());
repo!(AlertChannels, AlertLedger, |fakes| fakes.ledger.clone());
repo!(AlertDuty, AlertLedger, |fakes| fakes.ledger.clone());
repo!(ModelPrices, PriceLedger, |fakes| {
    let _ = fakes;
    PriceLedger::default()
});
repo!(Incidents, IncidentLedger, |fakes| {
    let _ = fakes;
    IncidentLedger::default()
});
repo!(IncidentTimeline, IncidentLedger, |fakes| {
    let _ = fakes;
    IncidentLedger::default()
});
repo!(Schedules, OncallLedger, |fakes| fakes.oncall.clone());
repo!(Pages, OncallLedger, |fakes| fakes.oncall.clone());
repo!(StatusOutbox, StatusLedger, |fakes| {
    let _ = fakes;
    StatusLedger::default()
});

struct TestStack;

impl Stack for TestStack {
    type Repos = Fakes;
    type Clock = FixedClock;
    type Mailer = Couriered;
    type Photos = NoPhotos;
    type SpanStore = Steady;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Oidc = Inert;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Couriered;
}

struct Harness {
    ledger: AlertLedger,
    notebook: Notebook,
    mailer: Couriered,
    hooks: Couriered,
    oncall: OncallLedger,
}

impl Harness {
    fn new() -> Self {
        Self {
            ledger: AlertLedger::sealed(),
            notebook: Notebook::default(),
            mailer: Couriered::default(),
            hooks: Couriered::default(),
            oncall: OncallLedger::default(),
        }
    }

    fn unsealed() -> Self {
        Self {
            ledger: AlertLedger::default(),
            ..Self::new()
        }
    }

    fn app(&self, role: OrgRole) -> Router {
        let adapters = Adapters {
            clock: FixedClock::at(NOW),
            mailer: self.mailer.clone(),
            webhooks: self.hooks.clone(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: Some(Steady),
            issue_store: Some(Inert::default()),
            log_store: None,
            metric_store: None,
            oidc: None,
            provision: ProvisionPolicy::default(),
        };
        alert_router(AppState::new(
            Arc::new(Ferristrace::<TestStack>::new(
                Fakes {
                    role,
                    ledger: self.ledger.clone(),
                    notebook: self.notebook.clone(),
                    oncall: self.oncall.clone(),
                },
                adapters,
            )),
            SessionCookie::default(),
        ))
    }

    async fn served(&self, role: OrgRole, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.app(role).oneshot(request).await.expect("a response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let value = serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
        (status, value)
    }

    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.served(OrgRole::Admin, as_session(method, uri, body))
            .await
    }
}

fn as_session(method: Method, uri: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-requested-with", "fetch")
        .header(COOKIE, format!("ft_session={TOKEN}"));
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

fn alerts(org: u128) -> String {
    format!("/api/orgs/{}/alerts", id(org))
}

fn channels(org: u128) -> String {
    format!("/api/orgs/{}/channels", id(org))
}

fn checkout(name: &str, project: Option<u128>, channels: &[&str]) -> Value {
    json!({
        "name": name,
        "project": project.map(id),
        "enabled": true,
        "signal": {"kind": "traces", "service": "checkout", "operation": null,
                   "environment": null, "measure": "error_rate"},
        "condition": {"op": ">", "threshold": 5},
        "window_s": 300,
        "pending_s": 0,
        "severity": "critical",
        "no_data": "ok",
        "channels": channels,
    })
}

async fn created(harness: &Harness, body: Value) -> Value {
    let (status, rule) = harness.admin(Method::POST, &alerts(MINE), Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{rule}");
    rule
}

async fn a_webhook(harness: &Harness) -> Value {
    let (status, channel) = harness
        .admin(
            Method::POST,
            &channels(MINE),
            Some(json!({"name": "pager", "kind": "webhook", "target": "https://hooks.example.com/in"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{channel}");
    channel
}

#[tokio::test]
async fn a_rule_is_created_read_patched_and_deleted_in_the_frozen_shape() {
    let harness = Harness::new();

    let rule = created(&harness, checkout("Checkout errors", None, &[])).await;
    let rule_id = rule["id"].as_str().expect("an id").to_owned();
    let (listed_status, listed) = harness.admin(Method::GET, &alerts(MINE), None).await;
    let (one_status, one) = harness
        .admin(Method::GET, &format!("{}/{rule_id}", alerts(MINE)), None)
        .await;
    let (patched_status, patched) = harness
        .admin(
            Method::PATCH,
            &format!("{}/{rule_id}", alerts(MINE)),
            Some(json!({"enabled": false})),
        )
        .await;
    let (bad_status, _) = harness
        .admin(
            Method::PATCH,
            &format!("{}/{rule_id}", alerts(MINE)),
            Some(json!({"window_s": 10})),
        )
        .await;
    let (deleted, _) = harness
        .admin(Method::DELETE, &format!("{}/{rule_id}", alerts(MINE)), None)
        .await;
    let (gone, _) = harness
        .admin(Method::GET, &format!("{}/{rule_id}", alerts(MINE)), None)
        .await;

    assert_eq!(rule["name"], "Checkout errors");
    assert_eq!(rule["project"], Value::Null);
    assert_eq!(rule["enabled"], true);
    assert_eq!(
        rule["signal"],
        json!({"kind": "traces", "service": "checkout", "operation": null,
               "environment": null, "measure": "error_rate"})
    );
    assert_eq!(rule["condition"], json!({"op": ">", "threshold": 5.0}));
    assert_eq!(rule["window_s"], 300);
    assert_eq!(rule["pending_s"], 0);
    assert_eq!(rule["severity"], "critical");
    assert_eq!(rule["no_data"], "ok");
    assert_eq!(rule["channels"], json!([]));
    assert_eq!(rule["state"]["status"], "ok");
    assert_eq!(rule["state"]["value"], Value::Null);
    assert!(rule["state"]["since_ms"].is_u64());
    assert_eq!(rule["state"]["evaluated_ms"], Value::Null);
    assert_eq!(rule["state"]["series"], json!([]));
    assert_eq!(listed_status, StatusCode::OK);
    assert_eq!(listed["alerts"][0]["id"], rule_id.as_str());
    assert_eq!(one_status, StatusCode::OK);
    assert_eq!(one, rule);
    assert_eq!(patched_status, StatusCode::OK);
    assert_eq!(patched["enabled"], false);
    assert_eq!(
        patched["window_s"], 300,
        "a patch keeps what it does not name"
    );
    assert_eq!(bad_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(deleted, StatusCode::NO_CONTENT);
    assert_eq!(gone, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_malformed_rule_is_refused_with_422() {
    let harness = Harness::new();
    let mut short = checkout("Short", None, &[]);
    short["window_s"] = json!(10);
    let mut unknown = checkout("Unknown", None, &[]);
    unknown["signal"]["measure"] = json!("p42");
    let mut severity = checkout("Severity", None, &[]);
    severity["severity"] = json!("sev1");
    let missing = checkout("Missing", None, &[&id(77)]);

    for body in [short, unknown, severity, missing] {
        let (status, refused) = harness.admin(Method::POST, &alerts(MINE), Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    }
    created(&harness, checkout("Twice", None, &[])).await;
    let (twice, _) = harness
        .admin(
            Method::POST,
            &alerts(MINE),
            Some(checkout("Twice", None, &[])),
        )
        .await;
    assert_eq!(twice, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn members_read_admins_write_and_strangers_find_nothing() {
    let harness = Harness::new();
    created(&harness, checkout("Checkout errors", None, &[])).await;

    let (member_reads, _) = harness
        .served(
            OrgRole::Member,
            as_session(Method::GET, &alerts(MINE), None),
        )
        .await;
    let (member_writes, _) = harness
        .served(
            OrgRole::Member,
            as_session(
                Method::POST,
                &alerts(MINE),
                Some(checkout("Other", None, &[])),
            ),
        )
        .await;
    let (member_channels, _) = harness
        .served(
            OrgRole::Member,
            as_session(
                Method::POST,
                &channels(MINE),
                Some(json!({"name": "ops", "kind": "email", "target": "ops@example.com"})),
            ),
        )
        .await;
    let (stranger, _) = harness.admin(Method::GET, &alerts(THEIRS), None).await;
    let (anonymous, _) = harness
        .served(
            OrgRole::Admin,
            Request::builder()
                .uri(alerts(MINE))
                .header("x-requested-with", "fetch")
                .body(Body::empty())
                .expect("a request"),
        )
        .await;

    assert_eq!(member_reads, StatusCode::OK);
    assert_eq!(member_writes, StatusCode::FORBIDDEN);
    assert_eq!(member_channels, StatusCode::FORBIDDEN);
    assert_eq!(stranger, StatusCode::NOT_FOUND);
    assert_eq!(anonymous, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_read_key_reads_and_a_project_bound_one_sees_only_its_project() {
    let harness = Harness::new();
    let ours = created(&harness, checkout("Ours", Some(OURS), &[])).await;
    let whole = created(&harness, checkout("Whole", None, &[])).await;

    let (bound_status, bound) = harness
        .served(OrgRole::Member, as_key(BOUND_KEY, &alerts(MINE)))
        .await;
    let (whole_status, every) = harness
        .served(OrgRole::Member, as_key(WHOLE_KEY, &alerts(MINE)))
        .await;
    let (hidden, _) = harness
        .served(
            OrgRole::Member,
            as_key(
                BOUND_KEY,
                &format!("{}/{}", alerts(MINE), whole["id"].as_str().expect("an id")),
            ),
        )
        .await;
    let (unknown, _) = harness
        .served(
            OrgRole::Member,
            as_key("ft_live_nope.secret", &alerts(MINE)),
        )
        .await;
    let (key_channels, _) = harness
        .served(OrgRole::Member, as_key(WHOLE_KEY, &channels(MINE)))
        .await;

    assert_eq!(bound_status, StatusCode::OK);
    assert_eq!(bound["alerts"].as_array().map(Vec::len), Some(1));
    assert_eq!(bound["alerts"][0]["id"], ours["id"]);
    assert_eq!(whole_status, StatusCode::OK);
    assert_eq!(every["alerts"].as_array().map(Vec::len), Some(2));
    assert_eq!(hidden, StatusCode::NOT_FOUND);
    assert_eq!(unknown, StatusCode::UNAUTHORIZED);
    assert_eq!(key_channels, StatusCode::OK);
}

#[tokio::test]
async fn a_preview_charts_the_definition_without_saving_it() {
    let harness = Harness::new();
    let mut body = checkout("Unsaved", None, &[]);
    body.as_object_mut().expect("an object").remove("name");

    let (status, preview) = harness
        .served(
            OrgRole::Member,
            as_session(
                Method::POST,
                &format!("{}/preview", alerts(MINE)),
                Some(body),
            ),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["step_ns"], 10_000_000_000_u64);
    let points = preview["points"].as_array().expect("points");
    assert_eq!(points.len(), 30);
    assert_eq!(points[29][0], NOW * 1_000_000_000);
    assert_eq!(
        points[29][1], 10.0,
        "ten errors in a hundred requests is ten percent"
    );
    assert_eq!(preview["threshold"], 5.0);
    assert_eq!(preview["would_fire"], true);
    assert!(harness.ledger.rules().is_empty());
}

#[tokio::test]
async fn a_test_sends_to_every_channel_and_reports_each_delivery() {
    let harness = Harness::new();
    let hook = a_webhook(&harness).await;
    let (_, mail) = harness
        .admin(
            Method::POST,
            &channels(MINE),
            Some(json!({"name": "ops", "kind": "email", "target": "ops@example.com"})),
        )
        .await;
    let rule = created(
        &harness,
        checkout(
            "Checkout errors",
            None,
            &[
                hook["id"].as_str().expect("an id"),
                mail["id"].as_str().expect("an id"),
            ],
        ),
    )
    .await;
    harness.hooks.fail(true);

    let (status, tested) = harness
        .admin(
            Method::POST,
            &format!(
                "{}/{}/test",
                alerts(MINE),
                rule["id"].as_str().expect("an id")
            ),
            None,
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{tested}");
    assert_eq!(
        tested,
        json!({"deliveries": [
            {"channel": hook["id"], "status": "failed", "error": "the far end answered 500"},
            {"channel": mail["id"], "status": "sent", "error": null},
        ]})
    );
    assert_eq!(harness.mailer.sent().len(), 1);
}

#[tokio::test]
async fn the_history_lists_changes_newest_first_within_the_asked_span() {
    let harness = Harness::new();
    let rule = created(&harness, checkout("Checkout errors", None, &[])).await;
    let rule_id = rule["id"].as_str().expect("an id");
    for (at_ms, to) in [
        (1_000, AlertStatus::Pending),
        (2_000, AlertStatus::Firing),
        (9_000, AlertStatus::Ok),
    ] {
        harness.ledger.put_event(AlertEvent {
            id: EventId::fresh(),
            rule: RuleId::new(Uuid::parse_str(rule_id).expect("a uuid")),
            org: TenantId::new(Uuid::from_u128(MINE)),
            labels: Labels::none(),
            from: AlertStatus::Ok,
            to,
            value: Some(7.5),
            at_ms,
            context: (at_ms == 2_000).then(|| {
                ferristrace_domain::Culprits::new(
                    vec![ferristrace_domain::CulpritPod {
                        name: "checkout-1".into(),
                        node: Some("node-a".into()),
                        count: 4,
                    }],
                    3,
                    vec!["node-a".into()],
                    vec!["1.4.2".into()],
                    &Labels::none(),
                )
            }),
        });
    }

    let (status, history) = harness
        .served(
            OrgRole::Member,
            as_session(
                Method::GET,
                &format!("{}/{rule_id}/history?from=500&to=5000", alerts(MINE)),
                None,
            ),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        history,
        json!({"events": [
            {
                "at_ms": 2000, "from": "ok", "to": "firing", "value": 7.5, "labels": {},
                "context": {
                    "pods": [{"name": "checkout-1", "node": "node-a", "count": 4}],
                    "others": 2,
                    "nodes": ["node-a"],
                    "versions": ["1.4.2"],
                    "labels": {},
                },
            },
            {"at_ms": 1000, "from": "ok", "to": "pending", "value": 7.5, "labels": {}},
        ]})
    );
}

#[tokio::test]
async fn a_webhook_secret_is_returned_once_and_never_listed() {
    let harness = Harness::new();

    let hook = a_webhook(&harness).await;
    let (_, listed) = harness.admin(Method::GET, &channels(MINE), None).await;

    let secret = hook["secret"].as_str().expect("a secret");
    assert_eq!(secret.len(), 64);
    assert_eq!(hook["kind"], "webhook");
    assert_eq!(hook["target"], "https://hooks.example.com/in");
    assert_eq!(hook["has_secret"], true);
    assert_eq!(
        listed,
        json!({"channels": [{"id": hook["id"], "name": "pager", "kind": "webhook",
                             "target": "https://hooks.example.com/in", "has_secret": true}]})
    );
}

#[tokio::test]
async fn a_private_webhook_or_one_without_a_seal_is_refused() {
    let sealed = Harness::new();
    let unsealed = Harness::unsealed();

    let (private, refused) = sealed
        .admin(
            Method::POST,
            &channels(MINE),
            Some(json!({"name": "local", "kind": "webhook", "target": "http://127.0.0.1:9000/in"})),
        )
        .await;
    let (no_seal, _) = unsealed
        .admin(
            Method::POST,
            &channels(MINE),
            Some(json!({"name": "pager", "kind": "webhook", "target": "https://hooks.example.com/in"})),
        )
        .await;
    let (bad_kind, _) = sealed
        .admin(
            Method::POST,
            &channels(MINE),
            Some(json!({"name": "sms", "kind": "sms", "target": "+33600000000"})),
        )
        .await;

    assert_eq!(private, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(no_seal, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad_kind, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_channel_is_patched_and_deleted() {
    let harness = Harness::new();
    let hook = a_webhook(&harness).await;
    let at = format!("{}/{}", channels(MINE), hook["id"].as_str().expect("an id"));

    let (patched_status, patched) = harness
        .admin(
            Method::PATCH,
            &at,
            Some(json!({"name": "paging", "target": "https://other.example.com/"})),
        )
        .await;
    let (deleted, _) = harness.admin(Method::DELETE, &at, None).await;
    let (again, _) = harness.admin(Method::DELETE, &at, None).await;

    assert_eq!(patched_status, StatusCode::OK, "{patched}");
    assert_eq!(patched["name"], "paging");
    assert_eq!(patched["target"], "https://other.example.com/");
    assert!(patched.get("secret").is_none());
    assert_eq!(deleted, StatusCode::NO_CONTENT);
    assert_eq!(again, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn spec_l2_8_every_rule_and_channel_write_is_audited() {
    let harness = Harness::new();
    let hook = a_webhook(&harness).await;
    let channel_at = format!("{}/{}", channels(MINE), hook["id"].as_str().expect("an id"));
    let rule = created(&harness, checkout("Checkout errors", None, &[])).await;
    let rule_at = format!("{}/{}", alerts(MINE), rule["id"].as_str().expect("an id"));

    harness
        .admin(Method::PATCH, &rule_at, Some(json!({"pending_s": 120})))
        .await;
    harness
        .admin(Method::PATCH, &rule_at, Some(json!({"enabled": false})))
        .await;
    harness
        .admin(Method::PATCH, &rule_at, Some(json!({"enabled": true})))
        .await;
    harness.admin(Method::DELETE, &rule_at, None).await;
    harness
        .admin(Method::PATCH, &channel_at, Some(json!({"name": "paging"})))
        .await;
    harness.admin(Method::DELETE, &channel_at, None).await;

    let entries = harness.notebook.entries();
    let written: Vec<(&str, TargetKind)> = entries
        .iter()
        .map(|entry| (entry.action.as_str(), entry.target.kind))
        .collect();
    assert_eq!(
        written,
        vec![
            ("channel.created", TargetKind::Channel),
            ("alert.created", TargetKind::Alert),
            ("alert.updated", TargetKind::Alert),
            ("alert.disabled", TargetKind::Alert),
            ("alert.enabled", TargetKind::Alert),
            ("alert.deleted", TargetKind::Alert),
            ("channel.updated", TargetKind::Channel),
            ("channel.deleted", TargetKind::Channel),
        ]
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.org == TenantId::new(Uuid::from_u128(MINE)))
    );
    assert_eq!(Action::parse("alert.disabled"), Some(Action::AlertDisabled));
}

fn a_schedule(harness: &Harness) -> String {
    let schedule = ferristrace_domain::Schedule {
        id: ferristrace_domain::ScheduleId::new(Uuid::from_u128(400)),
        org: TenantId::new(Uuid::from_u128(MINE)),
        team: ferristrace_domain::TeamId::new(Uuid::from_u128(300)),
        name: ferristrace_domain::ScheduleName::try_from("Primary").expect("a name"),
        timezone: ferristrace_domain::Timezone::utc(),
        rotation: ferristrace_domain::Rotation::new(Vec::new()).expect("a rotation"),
        handoff: ferristrace_domain::Handoff {
            weekday: ferristrace_domain::Weekday::Monday,
            time: ferristrace_domain::TimeOfDay::try_from("09:00").expect("a time"),
        },
        length: ferristrace_domain::RotationLength::try_from(7).expect("a length"),
        starts_on: ferristrace_domain::Day::try_from("2026-11-02").expect("a day"),
    };
    harness.oncall.put(schedule.clone());
    schedule.id.to_string()
}

#[tokio::test]
async fn a_rule_pages_the_on_call_of_a_schedule_in_the_frozen_shape() {
    let harness = Harness::new();
    let schedule = a_schedule(&harness);
    let mut paging = checkout("Paging", None, &[]);
    paging["page"] = json!({"schedule": schedule});
    let mut unknown = checkout("Unknown", None, &[]);
    unknown["page"] = json!({"schedule": id(999)});

    let silent = created(&harness, checkout("Silent", None, &[])).await;
    let rule = created(&harness, paging).await;
    let (refused, _) = harness
        .admin(Method::POST, &alerts(MINE), Some(unknown))
        .await;
    let rule_id = rule["id"].as_str().expect("an id").to_owned();
    let (_, kept) = harness
        .admin(
            Method::PATCH,
            &format!("{}/{rule_id}", alerts(MINE)),
            Some(json!({"enabled": false})),
        )
        .await;
    let (_, cleared) = harness
        .admin(
            Method::PATCH,
            &format!("{}/{rule_id}", alerts(MINE)),
            Some(json!({"page": null})),
        )
        .await;

    assert_eq!(silent["page"], Value::Null);
    assert_eq!(rule["page"], json!({"schedule": schedule}));
    assert_eq!(refused, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        kept["page"],
        json!({"schedule": schedule}),
        "a patch keeps the page"
    );
    assert_eq!(cleared["page"], Value::Null);
}

#[tokio::test]
async fn a_trace_or_log_rule_keeps_the_placement_it_watches() {
    let harness = Harness::new();
    let mut traced = checkout("Checkout pod errors", None, &[]);
    traced["signal"]["placement"] =
        json!({"namespace": "shop", "workload": "deployment:checkout", "pod": " checkout-1 "});
    let mut logged = checkout("Node errors", None, &[]);
    logged["signal"] = json!({"kind": "logs", "severities": ["error"],
                              "placement": {"node": "node-a", "cluster": "prod"}});

    let traced = created(&harness, traced).await;
    let logged = created(&harness, logged).await;

    assert_eq!(
        traced["signal"]["placement"],
        json!({"namespace": "shop", "workload": "deployment:checkout", "pod": "checkout-1"})
    );
    assert_eq!(
        logged["signal"]["placement"],
        json!({"cluster": "prod", "node": "node-a"})
    );
}

#[tokio::test]
async fn a_rule_watching_a_placement_that_is_no_kubernetes_name_is_refused_with_422() {
    let harness = Harness::new();
    let mut pod = checkout("Bad pod", None, &[]);
    pod["signal"]["placement"] = json!({"pod": "a b"});
    let mut kind = checkout("Bad kind", None, &[]);
    kind["signal"]["placement"] = json!({"workload": "replicaset:api"});

    for body in [pod, kind] {
        let (status, refused) = harness.admin(Method::POST, &alerts(MINE), Some(body)).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
        assert!(refused.to_string().contains("placement"), "{refused}");
    }
}

fn spend_rule(name: &str) -> Value {
    let mut body = checkout(name, None, &[]);
    body["signal"] = json!({
        "kind": "llm",
        "measure": "cost",
        "group_by": "feature",
        "filter": {"feature": " ticket-summary ", "customer": "acme", "project": id(30)},
    });
    body["condition"] = json!({"op": ">", "threshold": 10});
    body["window_s"] = json!(3600);
    body
}

#[tokio::test]
async fn an_llm_rule_is_created_read_back_and_patched_in_the_frozen_shape() {
    let harness = Harness::new();

    let made = created(&harness, spend_rule("Ticket spend")).await;

    assert_eq!(
        made["signal"],
        json!({
            "kind": "llm",
            "measure": "cost",
            "group_by": "feature",
            "filter": {"feature": "ticket-summary", "customer": "acme", "project": id(30)},
        })
    );
    let uri = format!("{}/{}", alerts(MINE), made["id"].as_str().expect("an id"));
    let (status, one) = harness.admin(Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert_eq!(one["signal"], made["signal"]);
    let (status, patched) = harness
        .admin(
            Method::PATCH,
            &uri,
            Some(json!({"signal": {"kind": "llm", "measure": "p95"}})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(
        patched["signal"],
        json!({"kind": "llm", "measure": "p95", "group_by": "none", "filter": {}})
    );
}

#[tokio::test]
async fn an_llm_rule_with_a_bad_measure_group_or_filter_is_refused_with_422() {
    let harness = Harness::new();
    let mut measure = spend_rule("Measure");
    measure["signal"]["measure"] = json!("spend");
    let mut group = spend_rule("Group");
    group["signal"]["group_by"] = json!("user");
    let mut blank = spend_rule("Blank");
    blank["signal"]["filter"] = json!({"model": "  "});
    let mut threshold = spend_rule("Threshold");
    threshold["condition"]["op"] = json!("~");

    for body in [measure, group, blank, threshold] {
        let (status, refused) = harness.admin(Method::POST, &alerts(MINE), Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    }
    assert!(harness.ledger.rules().is_empty());
}

#[tokio::test]
async fn a_preview_charts_an_llm_rule_without_saving_it() {
    let harness = Harness::new();
    let mut body = spend_rule("Unsaved");
    body.as_object_mut().expect("an object").remove("name");

    let (status, preview) = harness
        .served(
            OrgRole::Member,
            as_session(
                Method::POST,
                &format!("{}/preview", alerts(MINE)),
                Some(body),
            ),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{preview}");
    let points = preview["points"].as_array().expect("points");
    assert_eq!(points.len(), 30);
    assert!(points.iter().all(|point| point[1].is_null()));
    assert_eq!(preview["threshold"], 10.0);
    assert_eq!(preview["would_fire"], false);
    assert!(harness.ledger.rules().is_empty());
}
