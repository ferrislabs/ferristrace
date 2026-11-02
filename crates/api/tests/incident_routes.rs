use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, incident_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{
    AlertLedger, Crew, IncidentLedger, Inert, Notebook, OncallLedger, StatusLedger, Untransacted,
};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Action, Agent, AlertRules, AlertSeverity, AlertStates, AlertStatus, ApiKey, ApiKeys,
    ApiSecret, Assigned, Comparison, Condition, Created, Definition, DefinitionParts, IssuedKey,
    KeyId, KeyName, KeyScopes, Labels, LiveSession, Membership, Memberships, NoDataPolicy,
    NoPhotos, OrgRole, Project, ProjectId, ProjectName, Projects, ProvisionPolicy, ReadGrant,
    Renamed, RuleDraft, RuleId, RuleName, RuleState, SeriesState, Session, SessionError, SessionId,
    SessionRoll, SessionStore, SessionToken, Signal, StoreError, TargetKind, TeamId, TenantId,
    TraceMeasure, TraceSignal, UnixSeconds, WhoamiError,
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
            org: org(MINE),
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
        Ok(match secret.exposed() {
            BOUND_KEY => Some(ReadGrant {
                org: org(MINE),
                project: Some(ProjectId::new(Uuid::from_u128(OURS))),
            }),
            WHOLE_KEY => Some(ReadGrant::whole(org(MINE))),
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

struct Fakes {
    role: OrgRole,
    alerts: AlertLedger,
    incidents: IncidentLedger,
    crew: Crew,
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
repo!(Roster, Crew, |fakes| fakes.crew.clone());
repo!(AlertRules, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertStates, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertEvents, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertOutbox, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertChannels, AlertLedger, |fakes| fakes.alerts.clone());
repo!(AlertDuty, AlertLedger, |fakes| fakes.alerts.clone());
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
    type Hasher = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Oidc = Inert;
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
}

impl Harness {
    fn new() -> Self {
        let crew = Crew::default();
        crew.seat(org(MINE), account(ME), "person@example.com");
        crew.seat(org(MINE), account(COLLEAGUE), "colleague@example.com");
        crew.seat(org(THEIRS), account(STRANGER), "stranger@example.com");
        Self {
            alerts: AlertLedger::default(),
            incidents: IncidentLedger::default(),
            crew,
            notebook: Notebook::default(),
            oncall: OncallLedger::default(),
        }
    }

    fn app(&self, role: OrgRole) -> Router {
        let adapters = Adapters {
            clock: FixedClock::at(NOW),
            hasher: Inert::default(),
            mailer: Inert::default(),
            webhooks: Inert::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: None,
            issue_store: None,
            log_store: None,
            metric_store: None,
            oidc: None,
            provision: ProvisionPolicy::default(),
        };
        incident_router(AppState::new(
            Arc::new(Ferristrace::<TestStack>::new(
                Fakes {
                    role,
                    alerts: self.alerts.clone(),
                    incidents: self.incidents.clone(),
                    crew: self.crew.clone(),
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
