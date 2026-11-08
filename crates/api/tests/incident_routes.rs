use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, incident_router, mark_routes};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{
    AlertLedger, Bearers, Crew, IncidentLedger, Inert, Notebook, OncallLedger, PriceLedger,
    StatusLedger, Untransacted,
};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Action, Actor, AgentId, AgentScope, AgentScopes, AgentToken, AgentTokens,
    AlertRules, AlertSeverity, AlertStates, AlertStatus, ApiKey, ApiKeys, ApiSecret, Assigned,
    AuditEntry, AuditTrail, Comparison, Condition, Created, Definition, DefinitionParts,
    Delegation, IssuedKey, KeyId, KeyName, KeyScopes, Labels, Membership, Memberships,
    NoDataPolicy, NoPhotos, OrgRole, Project, ProjectId, ProjectName, Projects, ReadGrant,
    RegisteredAgent, Renamed, RuleDraft, RuleId, RuleName, RuleState, SeriesState, Session,
    SessionError, SessionStore, SessionToken, Signal, StoreError, TargetKind, TeamId, TenantId,
    TokenRefusal, TraceMeasure, TraceSignal, UnixSeconds, VerifiedToken, WhoamiError,
};
use ferristrace_infra::FixedClock;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft.person.0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const OURS: u128 = 30;
const ME: u128 = 9;
const COLLEAGUE: u128 = 10;
const STRANGER: u128 = 11;
const NOW: u64 = 1_700_000_000;
const BOUND_KEY: &str = "ft_live_bound.secret";
const WHOLE_KEY: &str = "ft_live_whole.secret";
const TRACE: &str = "0af7651916cd43dd8448eb211c80319c";

fn id(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

fn org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

#[derive(Clone)]
struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
            Ok(Session {
                account: account(ME),
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
}

struct MemberOfMine(OrgRole);

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: org(MINE),
            name: "Acme".into(),
            role: self.0,
        }])
    }
}

#[derive(Clone, Default)]
struct TwoKeys {
    agents: Arc<Mutex<Vec<RegisteredAgent>>>,
    notebook: Notebook,
}

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
        Ok(match secret.exposed() {
            BOUND_KEY => Some(ReadGrant {
                org: org(MINE),
                project: Some(ProjectId::new(Uuid::from_u128(OURS))),
            }),
            WHOLE_KEY => Some(ReadGrant::whole(org(MINE))),
            _ => None,
        })
    }

    async fn agent_standing(
        &self,
        org: TenantId,
        client_id: &str,
    ) -> Result<Option<RegisteredAgent>, StoreError> {
        Ok(self
            .agents
            .lock()
            .expect("the lock")
            .iter()
            .find(|agent| agent.org == org && &*agent.client_id == client_id)
            .cloned())
    }

    async fn agent_used(
        &self,
        _org: TenantId,
        _agent: AgentId,
        entry: &AuditEntry,
    ) -> Result<(), StoreError> {
        self.notebook.record(entry).await
    }
}

#[derive(Default)]
struct Tokens {
    known: Mutex<HashMap<String, VerifiedToken>>,
}

impl Tokens {
    fn mint(&self, name: &str, client_id: &str, scopes: &[&str], jti: &str) -> String {
        let token = format!("head.{name}.sig");
        self.known.lock().expect("the lock").insert(
            token.clone(),
            VerifiedToken {
                client_id: client_id.into(),
                scopes: scopes.iter().map(|scope| (*scope).into()).collect(),
                jti: jti.into(),
                expires_at_unix_secs: 4_000_000_000,
                delegated: None,
            },
        );
        token
    }
}

impl Tokens {
    fn delegate(&self, token: &str, subject: &str, email: Option<&str>) {
        if let Some(held) = self.known.lock().expect("the lock").get_mut(token) {
            held.delegated = Some(Delegation {
                subject: subject.into(),
                email: email.map(Into::into),
                act_sub: Some("gateway".into()),
                act_client_id: None,
            });
        }
    }
}

impl AgentTokens for Tokens {
    async fn verify(&self, token: &AgentToken) -> Result<VerifiedToken, TokenRefusal> {
        self.known
            .lock()
            .expect("the lock")
            .get(token.exposed())
            .cloned()
            .ok_or(TokenRefusal::Invalid)
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

struct Fakes {
    role: OrgRole,
    alerts: AlertLedger,
    incidents: IncidentLedger,
    crew: Crew,
    notebook: Notebook,
    oncall: OncallLedger,
    keys: TwoKeys,
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
repo!(ApiKeys, TwoKeys, |fakes| fakes.keys.clone());
repo!(Projects, OneProject, |fakes| {
    let _ = fakes;
    OneProject
});
repo!(Audit, Notebook, |fakes| fakes.notebook.clone());
repo!(Roster, Crew, |fakes| fakes.crew.clone());
repo!(AlertRules, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertStates, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertEvents, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertOutbox, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertChannels, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertDuty, AlertLedger, |fakes| fakes.alerts.clone());
repo!(ModelPrices, PriceLedger, |fakes| {
    let _ = fakes;
    PriceLedger::default()
});
repo!(Incidents, IncidentLedger, |fakes| fakes.incidents.clone());
repo!(IncidentTimeline, IncidentLedger, |fakes| fakes
    .incidents
    .clone());
repo!(IncidentAttachments, IncidentLedger, |fakes| fakes
    .incidents
    .clone());
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
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Users = Bearers;
    type Agents = Arc<Tokens>;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

struct Harness {
    alerts: AlertLedger,
    incidents: IncidentLedger,
    crew: Crew,
    notebook: Notebook,
    oncall: OncallLedger,
    keys: TwoKeys,
    tokens: Arc<Tokens>,
}

impl Harness {
    fn new() -> Self {
        let crew = Crew::default();
        crew.seat(org(MINE), account(ME), "person@example.com");
        crew.seat(org(MINE), account(COLLEAGUE), "colleague@example.com");
        crew.seat(org(THEIRS), account(STRANGER), "stranger@example.com");
        let notebook = Notebook::default();
        Self {
            alerts: AlertLedger::default(),
            incidents: IncidentLedger::default(),
            crew,
            keys: TwoKeys {
                agents: Arc::default(),
                notebook: notebook.clone(),
            },
            notebook,
            oncall: OncallLedger::default(),
            tokens: Arc::default(),
        }
    }

    fn app(&self, role: OrgRole) -> Router {
        let adapters = Adapters {
            clock: FixedClock::at(NOW),
            mailer: Inert::default(),
            webhooks: Inert::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: Some(Inert::default()),
            issue_store: Some(Inert::default()),
            log_store: None,
            metric_store: Some(Inert::default()),
            users: Some(Bearers),
        };
        let app = Ferristrace::<TestStack>::new(
            Fakes {
                role,
                alerts: self.alerts.clone(),
                incidents: self.incidents.clone(),
                crew: self.crew.clone(),
                notebook: self.notebook.clone(),
                oncall: self.oncall.clone(),
                keys: self.keys.clone(),
            },
            adapters,
        )
        .with_agent_tokens(Some(Arc::clone(&self.tokens)));
        mark_routes(incident_router(AppState::new(Arc::new(app))))
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

    async fn member(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.served(OrgRole::Member, as_session(method, uri, body))
            .await
    }

    async fn declared(&self, body: Value) -> Value {
        let (status, incident) = self
            .member(Method::POST, &incidents(MINE), Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{incident}");
        incident
    }

    fn enrol(&self, client_id: &str, scopes: &[AgentScope], project: Option<ProjectId>) -> AgentId {
        let mut agents = self.keys.agents.lock().expect("the lock");
        let agent = AgentId::new(Uuid::from_u128(500 + agents.len() as u128));
        agents.push(RegisteredAgent {
            id: agent,
            org: org(MINE),
            name: "triage bot".into(),
            client_id: client_id.into(),
            scopes: AgentScopes::new(scopes).expect("scopes"),
            project,
            created_by: None,
            created_at_unix_nanos: 1,
            last_used_at_unix_nanos: None,
            revoked_at_unix_nanos: None,
        });
        agent
    }

    fn revoke(&self, agent: AgentId) {
        for held in self.keys.agents.lock().expect("the lock").iter_mut() {
            if held.id == agent {
                held.revoked_at_unix_nanos = Some(2);
            }
        }
    }

    async fn agent_call(
        &self,
        token: &str,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.served(OrgRole::Member, as_key(token, method, uri, body))
            .await
    }

    async fn a_rule(&self, status: AlertStatus) -> RuleId {
        let rule = AlertRules::create(
            &self.alerts,
            org(MINE),
            &RuleDraft {
                name: RuleName::try_from("Checkout errors").expect("a name"),
                project: None,
                enabled: true,
                definition: Definition::new(DefinitionParts {
                    signal: Signal::Traces(TraceSignal {
                        service: Some("checkout".into()),
                        operation: None,
                        environment: None,
                        measure: TraceMeasure::ErrorRate,
                        placement: ferristrace_domain::Placement::default(),
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity: AlertSeverity::Critical,
                    no_data: NoDataPolicy::Ok,
                    channels: Vec::new(),
                    page: None,
                })
                .expect("a definition"),
            },
            account(ME),
        )
        .await
        .expect("kept")
        .expect("fresh");
        AlertStates::keep(
            &self.alerts,
            rule.id,
            &SeriesState {
                labels: Labels::none(),
                state: RuleState {
                    status,
                    value: Some(9.0),
                    since_ms: 1,
                    evaluated_ms: 1,
                },
            },
        )
        .await
        .expect("kept");
        rule.id
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

fn as_key(key: &str, method: Method, uri: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {key}"));
    match body {
        Some(body) => builder
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("a request")
}

fn incidents(org: u128) -> String {
    format!("/api/orgs/{}/incidents", id(org))
}

fn one(org: u128, incident: &Value) -> String {
    format!(
        "{}/{}",
        incidents(org),
        incident["id"].as_str().expect("an id")
    )
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn an_incident_is_declared_listed_read_and_annotated_in_the_documented_shape() {
    let harness = Harness::new();

    let incident = harness
        .declared(json!({"title": "Checkout down", "severity": "sev1", "project": id(OURS)}))
        .await;
    let path = one(MINE, &incident);
    let (commented_status, commented) = harness
        .member(
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "rolled back the deploy"})),
        )
        .await;
    let (attached_status, attached) = harness
        .member(
            Method::POST,
            &format!("{path}/attachments"),
            Some(json!({"kind": "trace", "reference": TRACE, "label": "a failing payment"})),
        )
        .await;
    let (listed_status, listed) = harness.member(Method::GET, &incidents(MINE), None).await;
    let (read_status, read) = harness.member(Method::GET, &path, None).await;
    let (timeline_status, timeline) = harness
        .member(Method::GET, &format!("{path}/timeline"), None)
        .await;

    assert_eq!(incident["title"], "Checkout down");
    assert_eq!(incident["severity"], "sev1");
    assert_eq!(incident["status"], "investigating");
    assert_eq!(incident["owner"], Value::Null);
    assert_eq!(incident["public"], false);
    assert_eq!(incident["project"], id(OURS));
    assert_eq!(incident["opened_ms"], NOW * 1_000);
    assert_eq!(incident["resolved_ms"], Value::Null);
    assert_eq!(incident["alert"], Value::Null);
    assert_eq!(commented_status, StatusCode::CREATED);
    assert_eq!(commented["kind"], "comment");
    assert_eq!(
        commented["detail"],
        json!({"body": "rolled back the deploy", "public": false})
    );
    assert_eq!(
        commented["author"],
        json!({"kind": "account", "id": id(ME), "label": "person@example.com"})
    );
    assert_eq!(attached_status, StatusCode::CREATED);
    assert_eq!(attached["kind"], "trace");
    assert_eq!(attached["reference"], TRACE);
    assert_eq!(attached["label"], "a failing payment");
    assert_eq!(listed_status, StatusCode::OK);
    assert_eq!(listed["incidents"], json!([incident]));
    assert_eq!(read_status, StatusCode::OK);
    assert_eq!(read["id"], incident["id"]);
    assert_eq!(read["title"], "Checkout down");
    assert_eq!(read["attachments"], json!([attached]));
    let kinds: Vec<&str> = read["timeline"]
        .as_array()
        .expect("a timeline")
        .iter()
        .map(|entry| entry["kind"].as_str().expect("a kind"))
        .collect();
    assert_eq!(kinds, vec!["opened", "comment", "attachment"]);
    assert_eq!(
        read["timeline"][0]["detail"],
        json!({"title": "Checkout down", "severity": "sev1", "alert": null})
    );
    assert_eq!(read["timeline"][0]["at_ms"], NOW * 1_000);
    assert_eq!(read["timeline"][2]["detail"]["reference"], TRACE);
    assert_eq!(timeline_status, StatusCode::OK);
    assert_eq!(timeline["timeline"], read["timeline"]);
}

#[tokio::test]
async fn spec_l3_5_an_alert_entry_carries_the_pods_nodes_and_version_that_fired() {
    use ferristrace_domain::{
        Author, Change, CulpritPod, Culprits, IncidentId, IncidentTimeline, TimelineEntry,
        TimelineId,
    };

    let harness = Harness::new();
    let rule = harness.a_rule(AlertStatus::Firing).await;
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    let id =
        IncidentId::new(Uuid::parse_str(incident["id"].as_str().expect("an id")).expect("a uuid"));
    for context in [
        Some(Culprits::new(
            vec![CulpritPod {
                name: "checkout-1".into(),
                node: Some("node-a".into()),
                count: 4,
            }],
            3,
            vec!["node-a".into()],
            vec!["1.4.2".into()],
            &Labels::none(),
        )),
        None,
    ] {
        IncidentTimeline::append(
            &harness.incidents,
            &TimelineEntry {
                id: TimelineId::fresh(),
                incident: id,
                org: org(MINE),
                author: Author::System,
                change: Change::Alert {
                    rule,
                    from: AlertStatus::Pending,
                    to: AlertStatus::Firing,
                    value: Some(9.0),
                    labels: Labels::none(),
                    context,
                },
                at_ms: NOW * 1_000,
            },
        )
        .await
        .expect("appended");
    }

    let (status, read) = harness
        .member(
            Method::GET,
            &format!("{}/timeline", one(MINE, &incident)),
            None,
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    let alerts: Vec<&Value> = read["timeline"]
        .as_array()
        .expect("a timeline")
        .iter()
        .filter(|entry| entry["kind"] == "alert")
        .collect();
    assert_eq!(alerts.len(), 2);
    assert_eq!(
        alerts[0]["detail"]["context"],
        json!({
            "pods": [{"name": "checkout-1", "node": "node-a", "count": 4}],
            "others": 2,
            "nodes": ["node-a"],
            "versions": ["1.4.2"],
            "labels": {},
        })
    );
    assert!(alerts[1]["detail"].get("context").is_none());
}

#[tokio::test]
async fn a_patch_writes_one_entry_per_field_it_changes() {
    let harness = Harness::new();
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    let path = one(MINE, &incident);

    let (patched_status, patched) = harness
        .member(
            Method::PATCH,
            &path,
            Some(json!({"severity": "sev2", "status": "identified", "owner": id(COLLEAGUE)})),
        )
        .await;
    let (same_status, _) = harness
        .member(
            Method::PATCH,
            &path,
            Some(json!({"severity": "sev2", "public": false})),
        )
        .await;
    let (empty_status, _) = harness.member(Method::PATCH, &path, Some(json!({}))).await;
    let (resolved_status, resolved) = harness
        .member(
            Method::PATCH,
            &path,
            Some(json!({"status": "resolved", "owner": null, "title": "Checkout was down"})),
        )
        .await;
    let (_, read) = harness.member(Method::GET, &path, None).await;

    assert_eq!(patched_status, StatusCode::OK);
    assert_eq!(patched["severity"], "sev2");
    assert_eq!(patched["status"], "identified");
    assert_eq!(
        patched["owner"],
        json!({"id": id(COLLEAGUE), "email": "colleague@example.com", "name": null})
    );
    assert_eq!(same_status, StatusCode::OK);
    assert_eq!(empty_status, StatusCode::OK);
    assert_eq!(resolved_status, StatusCode::OK);
    assert_eq!(resolved["resolved_ms"], NOW * 1_000);
    assert_eq!(resolved["owner"], Value::Null);
    let entries: Vec<(&str, &Value)> = read["timeline"]
        .as_array()
        .expect("a timeline")
        .iter()
        .map(|entry| (entry["kind"].as_str().expect("a kind"), &entry["detail"]))
        .collect();
    assert_eq!(
        entries,
        vec![
            (
                "opened",
                &json!({"title": "Checkout down", "severity": "sev3", "alert": null})
            ),
            ("severity", &json!({"from": "sev3", "to": "sev2"})),
            (
                "status",
                &json!({"from": "investigating", "to": "identified"})
            ),
            (
                "owner",
                &json!({"from": null, "to": {"id": id(COLLEAGUE), "email": "colleague@example.com", "name": null}})
            ),
            (
                "title",
                &json!({"from": "Checkout down", "to": "Checkout was down"})
            ),
            ("status", &json!({"from": "identified", "to": "resolved"})),
            (
                "owner",
                &json!({"from": {"id": id(COLLEAGUE), "email": "colleague@example.com", "name": null}, "to": null})
            ),
        ]
    );
}

#[tokio::test]
async fn an_incident_declared_from_a_firing_alert_names_it() {
    let harness = Harness::new();
    let firing = harness.a_rule(AlertStatus::Firing).await;

    let incident = harness.declared(json!({"alert": firing.to_string()})).await;
    let (_, read) = harness
        .member(Method::GET, &one(MINE, &incident), None)
        .await;

    assert_eq!(incident["title"], "Checkout errors");
    assert_eq!(incident["severity"], "sev2");
    assert_eq!(
        incident["alert"],
        json!({"id": firing.to_string(), "name": "Checkout errors"})
    );
    assert_eq!(
        read["timeline"][0]["detail"]["alert"],
        json!({"id": firing.to_string(), "name": "Checkout errors"})
    );
}

#[tokio::test]
async fn an_invalid_write_is_refused_with_422() {
    let harness = Harness::new();
    let quiet = harness.a_rule(AlertStatus::Ok).await;
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    let path = one(MINE, &incident);

    let refused = [
        harness
            .member(Method::POST, &incidents(MINE), Some(json!({"title": "  "})))
            .await,
        harness
            .member(
                Method::POST,
                &incidents(MINE),
                Some(json!({"title": "x", "severity": "critical"})),
            )
            .await,
        harness
            .member(
                Method::POST,
                &incidents(MINE),
                Some(json!({"alert": quiet.to_string()})),
            )
            .await,
        harness
            .member(
                Method::POST,
                &incidents(MINE),
                Some(json!({"title": "x", "project": id(77)})),
            )
            .await,
        harness
            .member(Method::PATCH, &path, Some(json!({"status": "closed"})))
            .await,
        harness
            .member(Method::PATCH, &path, Some(json!({"owner": id(STRANGER)})))
            .await,
        harness
            .member(
                Method::POST,
                &format!("{path}/comments"),
                Some(json!({"body": ""})),
            )
            .await,
        harness
            .member(
                Method::POST,
                &format!("{path}/attachments"),
                Some(json!({"kind": "trace", "reference": "nope", "label": "x"})),
            )
            .await,
        harness
            .member(
                Method::POST,
                &format!("{path}/attachments"),
                Some(json!({"kind": "profile", "reference": "x", "label": "x"})),
            )
            .await,
    ];
    let (listing, _) = harness
        .member(
            Method::GET,
            &format!("{}?status=later", incidents(MINE)),
            None,
        )
        .await;

    for (status, body) in refused {
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    assert_eq!(listing, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(harness.incidents.incidents().len(), 1);
    assert_eq!(harness.incidents.timeline().len(), 1);
}

#[tokio::test]
async fn a_read_key_reads_but_never_writes_and_a_stranger_finds_nothing() {
    let harness = Harness::new();
    let ours = harness
        .declared(json!({"title": "Ours", "project": id(OURS)}))
        .await;
    let whole = harness.declared(json!({"title": "Whole"})).await;
    let path = one(MINE, &whole);

    let (read, _) = harness
        .served(OrgRole::Member, as_key(WHOLE_KEY, Method::GET, &path, None))
        .await;
    let (bound_status, bound) = harness
        .served(
            OrgRole::Member,
            as_key(BOUND_KEY, Method::GET, &incidents(MINE), None),
        )
        .await;
    let (bound_hidden, _) = harness
        .served(OrgRole::Member, as_key(BOUND_KEY, Method::GET, &path, None))
        .await;
    let mut writes = Vec::new();
    for (method, uri, body) in [
        (Method::POST, incidents(MINE), json!({"title": "By key"})),
        (Method::PATCH, path.clone(), json!({"status": "resolved"})),
        (
            Method::POST,
            format!("{path}/comments"),
            json!({"body": "hi"}),
        ),
        (
            Method::POST,
            format!("{path}/attachments"),
            json!({"kind": "logs", "reference": "q=x", "label": "x"}),
        ),
    ] {
        writes.push(
            harness
                .served(OrgRole::Member, as_key(WHOLE_KEY, method, &uri, Some(body)))
                .await
                .0,
        );
    }
    let (stranger_reads, _) = harness.member(Method::GET, &incidents(THEIRS), None).await;
    let (stranger_writes, _) = harness
        .member(
            Method::POST,
            &incidents(THEIRS),
            Some(json!({"title": "Theirs"})),
        )
        .await;
    let (missing, _) = harness
        .member(
            Method::GET,
            &format!("{}/{}", incidents(MINE), id(404)),
            None,
        )
        .await;

    assert_eq!(read, StatusCode::OK);
    assert_eq!(bound_status, StatusCode::OK);
    assert_eq!(
        bound["incidents"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|incident| incident["id"].clone())
            .collect::<Vec<_>>(),
        vec![ours["id"].clone()]
    );
    assert_eq!(bound_hidden, StatusCode::NOT_FOUND);
    assert_eq!(writes, vec![StatusCode::FORBIDDEN; 4]);
    assert_eq!(stranger_reads, StatusCode::NOT_FOUND);
    assert_eq!(stranger_writes, StatusCode::NOT_FOUND);
    assert_eq!(missing, StatusCode::NOT_FOUND);
    assert_eq!(harness.incidents.incidents().len(), 2);
}

#[tokio::test]
async fn the_list_filters_open_and_resolved_incidents() {
    let harness = Harness::new();
    let open = harness.declared(json!({"title": "Open"})).await;
    let closed = harness.declared(json!({"title": "Closed"})).await;
    harness
        .member(
            Method::PATCH,
            &one(MINE, &closed),
            Some(json!({"status": "resolved"})),
        )
        .await;

    let ids = |listed: &Value| -> Vec<Value> {
        listed["incidents"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|incident| incident["id"].clone())
            .collect()
    };
    let (_, by_default) = harness.member(Method::GET, &incidents(MINE), None).await;
    let (_, resolved) = harness
        .member(
            Method::GET,
            &format!("{}?status=resolved", incidents(MINE)),
            None,
        )
        .await;
    let (_, all) = harness
        .member(
            Method::GET,
            &format!("{}?status=all", incidents(MINE)),
            None,
        )
        .await;

    assert_eq!(ids(&by_default), vec![open["id"].clone()]);
    assert_eq!(ids(&resolved), vec![closed["id"].clone()]);
    assert_eq!(all["incidents"].as_array().expect("a list").len(), 2);
}

#[tokio::test]
async fn spec_l2_8_every_incident_write_is_audited() {
    let harness = Harness::new();
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    let path = one(MINE, &incident);
    harness
        .member(
            Method::PATCH,
            &path,
            Some(json!({"severity": "sev1", "public": true})),
        )
        .await;
    harness
        .member(
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "looking"})),
        )
        .await;
    harness
        .member(
            Method::POST,
            &format!("{path}/attachments"),
            Some(json!({"kind": "metric", "reference": "metric=queue.depth", "label": "depth"})),
        )
        .await;
    harness
        .member(Method::PATCH, &path, Some(json!({"status": "resolved"})))
        .await;
    harness
        .member(Method::PATCH, &path, Some(json!({"status": "monitoring"})))
        .await;

    let entries = harness.notebook.entries();
    let actions: Vec<Action> = entries.iter().map(|entry| entry.action).collect();
    assert_eq!(
        actions,
        vec![
            Action::IncidentOpened,
            Action::IncidentUpdated,
            Action::IncidentCommented,
            Action::IncidentAttached,
            Action::IncidentResolved,
            Action::IncidentReopened,
        ]
    );
    assert!(entries.iter().all(|entry| {
        entry.target.kind == TargetKind::Incident
            && entry.target.id.as_deref() == incident["id"].as_str()
    }));
}

#[tokio::test]
async fn spec_l2_5_declaring_with_paging_pages_the_on_call_and_says_so() {
    let harness = Harness::new();
    harness.oncall.put(ferristrace_domain::Schedule {
        id: ferristrace_domain::ScheduleId::new(Uuid::from_u128(400)),
        org: org(MINE),
        team: TeamId::new(Uuid::from_u128(300)),
        name: ferristrace_domain::ScheduleName::try_from("Primary").expect("a name"),
        timezone: ferristrace_domain::Timezone::utc(),
        rotation: ferristrace_domain::Rotation::new(vec![account(COLLEAGUE), account(ME)])
            .expect("a rotation"),
        handoff: ferristrace_domain::Handoff {
            weekday: ferristrace_domain::Weekday::Monday,
            time: ferristrace_domain::TimeOfDay::try_from("00:00").expect("a time"),
        },
        length: ferristrace_domain::RotationLength::try_from(1).expect("a length"),
        starts_on: ferristrace_domain::Day::try_from("2023-01-02").expect("a day"),
    });

    let incident = harness
        .declared(json!({"title": "Checkout down", "page": {"schedule": id(400)}}))
        .await;
    let (refused, _) = harness
        .member(
            Method::POST,
            &incidents(MINE),
            Some(json!({"title": "Nowhere", "page": {"schedule": id(999)}})),
        )
        .await;
    let incident_id = incident["id"].as_str().expect("an id").to_owned();
    let (_, timeline) = harness
        .member(
            Method::GET,
            &format!("{}/{incident_id}/timeline", incidents(MINE)),
            None,
        )
        .await;

    assert_eq!(incident["paged_schedule"], json!(id(400)));
    assert_eq!(refused, StatusCode::UNPROCESSABLE_ENTITY);
    let pages = harness.oncall.pages();
    assert_eq!(pages.len(), 1);
    let entries = timeline["timeline"].as_array().expect("entries");
    assert_eq!(entries.len(), 2, "{timeline}");
    assert_eq!(entries[1]["kind"], "paged");
    assert_eq!(entries[1]["author"]["kind"], "system");
    assert_eq!(entries[1]["detail"]["page"], json!(pages[0].id.to_string()));
    assert_eq!(entries[1]["detail"]["step"], 0);
    assert_eq!(
        entries[1]["detail"]["target"]["id"],
        json!(pages[0].target.as_uuid().to_string())
    );
}

fn actions_of(harness: &Harness) -> Vec<Action> {
    harness
        .notebook
        .entries()
        .into_iter()
        .map(|entry| entry.action)
        .collect()
}

#[tokio::test]
async fn spec_l5_4_an_agent_with_the_comment_scope_comments_and_the_timeline_names_it() {
    let harness = Harness::new();
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    let path = one(MINE, &incident);
    let agent = harness.enrol(
        "bot",
        &[AgentScope::TelemetryRead, AgentScope::IncidentsComment],
        None,
    );
    let token = harness.tokens.mint(
        "commenting",
        "bot",
        &["telemetry:read", "incidents:comment"],
        "jti-42",
    );

    let (status, entry) = harness
        .agent_call(
            &token,
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "rolled back the release"})),
        )
        .await;
    let (_, timeline) = harness
        .agent_call(&token, Method::GET, &format!("{path}/timeline"), None)
        .await;

    assert_eq!(status, StatusCode::CREATED, "{entry}");
    assert_eq!(entry["author"]["kind"], "agent");
    assert_eq!(entry["author"]["label"], "triage bot");
    assert_eq!(entry["author"]["id"], agent.as_uuid().to_string());
    assert_eq!(entry["detail"]["public"], false);
    let last = timeline["timeline"]
        .as_array()
        .expect("a list")
        .last()
        .expect("an entry");
    assert_eq!(last["kind"], "comment");
    assert_eq!(last["author"]["kind"], "agent");
    assert_eq!(last["author"]["label"], "triage bot");
    let audited = harness
        .notebook
        .entries()
        .into_iter()
        .find(|entry| entry.action == Action::IncidentCommented)
        .expect("the comment is audited");
    assert_eq!(audited.actor, Actor::Agent(agent));
    assert_eq!(audited.detail.get("jti"), Some(Some("jti-42")));
}

#[tokio::test]
async fn spec_l5_4_resolving_needs_the_resolve_scope() {
    let harness = Harness::new();
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    let path = one(MINE, &incident);
    let agent = harness.enrol(
        "bot",
        &[AgentScope::IncidentsComment, AgentScope::IncidentsResolve],
        None,
    );
    let commenting = harness
        .tokens
        .mint("commenting", "bot", &["incidents:comment"], "jti-1");
    let resolving = harness
        .tokens
        .mint("resolving", "bot", &["incidents:resolve"], "jti-2");

    let (refused, _) = harness
        .agent_call(
            &commenting,
            Method::PATCH,
            &path,
            Some(json!({"status": "resolved"})),
        )
        .await;
    let audits_after_refusal = harness.notebook.entries().len();
    let (allowed, resolved) = harness
        .agent_call(
            &resolving,
            Method::PATCH,
            &path,
            Some(json!({"status": "resolved"})),
        )
        .await;

    assert_eq!(refused, StatusCode::FORBIDDEN);
    assert_eq!(audits_after_refusal, 1, "only the person's declaration");
    assert_eq!(allowed, StatusCode::OK, "{resolved}");
    assert_eq!(resolved["status"], "resolved");
    let audited = harness
        .notebook
        .entries()
        .into_iter()
        .find(|entry| entry.action == Action::IncidentResolved)
        .expect("audited");
    assert_eq!(audited.actor, Actor::Agent(agent));
    assert_eq!(audited.detail.get("jti"), Some(Some("jti-2")));
}

#[tokio::test]
async fn an_agent_cannot_change_anything_else_in_the_same_patch_or_on_its_own() {
    let harness = Harness::new();
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    let path = one(MINE, &incident);
    harness.enrol(
        "bot",
        &[AgentScope::IncidentsComment, AgentScope::IncidentsResolve],
        None,
    );
    let token = harness.tokens.mint(
        "all",
        "bot",
        &["telemetry:read", "incidents:comment", "incidents:resolve"],
        "jti-1",
    );
    let before = (
        harness.notebook.entries().len(),
        harness.incidents.timeline().len(),
    );

    let mut refused = Vec::new();
    for patch in [
        json!({"status": "resolved", "severity": "sev1"}),
        json!({"status": "resolved", "owner": id(ME)}),
        json!({"status": "resolved", "owner": null}),
        json!({"status": "resolved", "title": "Renamed"}),
        json!({"status": "resolved", "public": true}),
        json!({"status": "monitoring"}),
        json!({"status": "investigating"}),
        json!({"severity": "sev1"}),
        json!({"public": true}),
        json!({}),
    ] {
        refused.push(
            harness
                .agent_call(&token, Method::PATCH, &path, Some(patch))
                .await
                .0,
        );
    }
    let (public_comment, _) = harness
        .agent_call(
            &token,
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "to the world", "public": true})),
        )
        .await;
    let (opened, _) = harness
        .agent_call(
            &token,
            Method::POST,
            &incidents(MINE),
            Some(json!({"title": "Another"})),
        )
        .await;
    let (attached, _) = harness
        .agent_call(
            &token,
            Method::POST,
            &format!("{path}/attachments"),
            Some(json!({"kind": "trace", "reference": TRACE, "label": "slow"})),
        )
        .await;
    let (_, after) = harness.member(Method::GET, &path, None).await;

    assert!(
        refused
            .iter()
            .all(|status| *status == StatusCode::FORBIDDEN),
        "{refused:?}"
    );
    assert_eq!(public_comment, StatusCode::FORBIDDEN);
    assert_eq!(opened, StatusCode::FORBIDDEN);
    assert_eq!(attached, StatusCode::FORBIDDEN);
    assert_eq!(after["status"], "investigating");
    assert_eq!(after["severity"], incident["severity"]);
    assert_eq!(after["public"], false);
    assert_eq!(
        (
            harness.notebook.entries().len(),
            harness.incidents.timeline().len()
        ),
        before,
        "no refusal leaves an audit or a timeline entry"
    );
    assert_eq!(harness.incidents.incidents().len(), 1);
}

#[tokio::test]
async fn an_agent_out_of_its_scopes_project_or_standing_is_refused() {
    let harness = Harness::new();
    let ours = harness
        .declared(json!({"title": "Ours", "project": id(OURS)}))
        .await;
    let whole = harness.declared(json!({"title": "Whole"})).await;
    let both = [AgentScope::IncidentsComment, AgentScope::IncidentsResolve];
    harness.enrol("bound", &both, Some(ProjectId::new(Uuid::from_u128(OURS))));
    harness.enrol("reader", &[AgentScope::TelemetryRead], None);
    let revoked = harness.enrol("gone", &both, None);
    harness.revoke(revoked);
    let claims = ["incidents:comment", "incidents:resolve"];
    let bound = harness.tokens.mint("bound", "bound", &claims, "jti-1");
    let reader = harness.tokens.mint("reader", "reader", &claims, "jti-2");
    let gone = harness.tokens.mint("gone", "gone", &claims, "jti-3");
    let nobody = harness.tokens.mint("nobody", "nobody", &claims, "jti-4");
    let before = (
        harness.notebook.entries().len(),
        harness.incidents.timeline().len(),
    );

    let comment = |incident: &Value| {
        (
            format!("{}/comments", one(MINE, incident)),
            json!({"body": "hi"}),
        )
    };
    let mut statuses = Vec::new();
    for (token, incident) in [
        (&bound, &whole),
        (&reader, &ours),
        (&gone, &ours),
        (&nobody, &ours),
    ] {
        let (path, body) = comment(incident);
        statuses.push(
            harness
                .agent_call(token, Method::POST, &path, Some(body))
                .await
                .0,
        );
    }
    let (bound_resolve_other, _) = harness
        .agent_call(
            &bound,
            Method::PATCH,
            &one(MINE, &whole),
            Some(json!({"status": "resolved"})),
        )
        .await;
    let (own, _) = harness
        .agent_call(
            &bound,
            Method::POST,
            &format!("{}/comments", one(MINE, &ours)),
            Some(json!({"body": "hi"})),
        )
        .await;
    let (stranger, _) = harness
        .agent_call(
            &bound,
            Method::POST,
            &format!(
                "{}/{}/comments",
                incidents(THEIRS),
                whole["id"].as_str().expect("an id")
            ),
            Some(json!({"body": "hi"})),
        )
        .await;

    assert_eq!(
        statuses,
        vec![
            StatusCode::NOT_FOUND,
            StatusCode::FORBIDDEN,
            StatusCode::UNAUTHORIZED,
            StatusCode::UNAUTHORIZED
        ]
    );
    assert_eq!(bound_resolve_other, StatusCode::NOT_FOUND);
    assert_eq!(own, StatusCode::CREATED);
    assert_eq!(stranger, StatusCode::UNAUTHORIZED);
    assert_eq!(harness.incidents.timeline().len(), before.1 + 1);
    assert_eq!(harness.notebook.entries().len(), before.0 + 1);
    assert_eq!(
        actions_of(&harness).last(),
        Some(&Action::IncidentCommented)
    );
}

#[tokio::test]
async fn a_token_the_issuer_would_not_vouch_for_never_reaches_an_incident() {
    let harness = Harness::new();
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    harness.enrol("bot", &[AgentScope::IncidentsComment], None);

    let (status, _) = harness
        .agent_call(
            "head.forged.sig",
            Method::POST,
            &format!("{}/comments", one(MINE, &incident)),
            Some(json!({"body": "hi"})),
        )
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(harness.incidents.timeline().len(), 1);
}

#[tokio::test]
async fn spec_l7_4_a_delegated_resolve_names_the_person_in_the_timeline_and_the_audit_log() {
    let harness = Harness::new();
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    let path = one(MINE, &incident);
    let agent = harness.enrol(
        "bot",
        &[AgentScope::IncidentsComment, AgentScope::IncidentsResolve],
        None,
    );
    let commenting = harness.tokens.mint(
        "commenting",
        "bot",
        &["telemetry:read", "incidents:comment"],
        "jti-1",
    );
    let resolving = harness
        .tokens
        .mint("resolving", "bot", &["incidents:resolve"], "jti-2");
    for token in [&commenting, &resolving] {
        harness
            .tokens
            .delegate(token, "sub-dana", Some("dana@example.com"));
    }

    let (commented, entry) = harness
        .agent_call(
            &commenting,
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "rolling back"})),
        )
        .await;
    let (resolved, _) = harness
        .agent_call(
            &resolving,
            Method::PATCH,
            &path,
            Some(json!({"status": "resolved"})),
        )
        .await;
    let (_, read) = harness.member(Method::GET, &path, None).await;

    assert_eq!(commented, StatusCode::CREATED, "{entry}");
    assert_eq!(entry["author"]["on_behalf_of"], "dana@example.com");
    assert_eq!(resolved, StatusCode::OK);
    let timeline = read["timeline"].as_array().expect("a list");
    let by_agent: Vec<_> = timeline
        .iter()
        .filter(|entry| entry["author"]["kind"] == "agent")
        .collect();
    assert_eq!(by_agent.len(), 2);
    for entry in &by_agent {
        assert_eq!(entry["author"]["label"], "triage bot");
        assert_eq!(entry["author"]["on_behalf_of"], "dana@example.com");
    }
    let first = timeline.first().expect("the opening");
    assert!(first["author"].get("on_behalf_of").is_none());
    let audited: Vec<_> = harness
        .notebook
        .entries()
        .into_iter()
        .filter(|entry| entry.actor == Actor::Agent(agent))
        .collect();
    assert_eq!(audited.len(), 2);
    for entry in audited {
        assert_eq!(entry.detail.get("on_behalf_of"), Some(Some("sub-dana")));
        assert_eq!(
            entry.detail.get("on_behalf_of_email"),
            Some(Some("dana@example.com"))
        );
    }
}

#[tokio::test]
async fn an_incident_tells_its_severity_status_and_the_services_its_alert_concerns() {
    let harness = Harness::new();
    let firing = harness.a_rule(AlertStatus::Firing).await;
    let from_alert = harness.declared(json!({"alert": firing.to_string()})).await;
    let by_hand = harness.declared(json!({"title": "Disk full"})).await;
    harness.enrol("bot", &[AgentScope::TelemetryRead], None);
    let token = harness
        .tokens
        .mint("reader", "bot", &["telemetry:read"], "jti-1");

    let (status, read) = harness
        .agent_call(&token, Method::GET, &one(MINE, &from_alert), None)
        .await;
    let (_, manual) = harness
        .agent_call(&token, Method::GET, &one(MINE, &by_hand), None)
        .await;

    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["severity"], "sev2");
    assert_eq!(read["status"], "investigating");
    assert_eq!(read["services"], json!(["checkout"]));
    assert_eq!(manual["services"], json!([]));
}

#[tokio::test]
async fn spec_l13_4_an_incident_without_a_service_answers_an_empty_context() {
    let harness = Harness::new();
    let incident = harness.declared(json!({"title": "Plain"})).await;

    let (status, body) = harness
        .member(
            Method::GET,
            &format!("{}/context", one(MINE, &incident)),
            None,
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["services"], json!([]));
    for section in ["deploys", "issues", "resources", "traces"] {
        assert_eq!(body[section], json!([]), "{section}");
        assert_eq!(body["left_out"][section], 0, "{section}");
    }
    let from = body["from_ms"].as_u64().expect("a start");
    let to = body["to_ms"].as_u64().expect("an end");
    assert!(to > from);
}

#[tokio::test]
async fn spec_l13_5_a_read_key_bound_elsewhere_and_a_stranger_see_no_context() {
    let harness = Harness::new();
    let whole = harness.declared(json!({"title": "Whole"})).await;
    let path = format!("{}/context", one(MINE, &whole));

    let (bound, _) = harness
        .served(OrgRole::Member, as_key(BOUND_KEY, Method::GET, &path, None))
        .await;
    let (whole_key, _) = harness
        .served(OrgRole::Member, as_key(WHOLE_KEY, Method::GET, &path, None))
        .await;
    let (missing, _) = harness
        .member(
            Method::GET,
            &format!("{}/{}/context", incidents(MINE), id(404)),
            None,
        )
        .await;
    let (stranger, _) = harness
        .member(
            Method::GET,
            &format!("{}/{}/context", incidents(THEIRS), id(404)),
            None,
        )
        .await;

    assert_eq!(bound, StatusCode::NOT_FOUND);
    assert_eq!(whole_key, StatusCode::OK);
    assert_eq!(missing, StatusCode::NOT_FOUND);
    assert_eq!(stranger, StatusCode::NOT_FOUND);
}
