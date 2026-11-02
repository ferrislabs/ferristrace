use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, incident_router, postmortem_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{
    AlertLedger, Crew, IncidentLedger, Inert, Notebook, OncallLedger, PostmortemLedger,
    PriceLedger, StatusLedger, Untransacted,
};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Action, Agent, ApiKey, ApiKeys, ApiSecret, Assigned, Created, IssuedKey, KeyId,
    KeyName, KeyScopes, LiveSession, Membership, Memberships, NoPhotos, OrgRole, Project,
    ProjectId, ProjectName, Projects, ProvisionPolicy, ReadGrant, Renamed, Session, SessionError,
    SessionId, SessionRoll, SessionStore, SessionToken, StoreError, TargetKind, TeamId, TenantId,
    UnixSeconds, WhoamiError,
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
    postmortems: PostmortemLedger,
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
repo!(Postmortems, PostmortemLedger, |fakes| fakes
    .postmortems
    .clone());

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
    postmortems: PostmortemLedger,
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
            postmortems: PostmortemLedger::default(),
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
        let state = AppState::new(
            Arc::new(Ferristrace::<TestStack>::new(
                Fakes {
                    role,
                    alerts: self.alerts.clone(),
                    incidents: self.incidents.clone(),
                    crew: self.crew.clone(),
                    notebook: self.notebook.clone(),
                    oncall: self.oncall.clone(),
                    postmortems: self.postmortems.clone(),
                },
                adapters,
            )),
            SessionCookie::default(),
        );
        incident_router(state.clone()).merge(postmortem_router(state))
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

    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.served(OrgRole::Admin, as_session(method, uri, body))
            .await
    }

    async fn declared(&self, body: Value) -> Value {
        let (status, incident) = self
            .member(Method::POST, &incidents(MINE), Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{incident}");
        incident
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

fn postmortem(org: u128, incident: &Value) -> String {
    format!("{}/postmortem", one(org, incident))
}

async fn a_resolved_incident(harness: &Harness) -> Value {
    let incident = harness.declared(json!({"title": "Checkout down"})).await;
    let path = one(MINE, &incident);
    harness
        .member(
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "rolled back the deploy"})),
        )
        .await;
    harness
        .member(
            Method::POST,
            &format!("{path}/attachments"),
            Some(json!({"kind": "trace", "reference": TRACE, "label": "a failing payment"})),
        )
        .await;
    let (status, _) = harness
        .member(Method::PATCH, &path, Some(json!({"status": "resolved"})))
        .await;
    assert_eq!(status, StatusCode::OK);
    incident
}

#[tokio::test]
async fn spec_l2_6_resolving_an_incident_offers_a_draft_with_its_window_and_timeline() {
    let harness = Harness::new();
    let incident = a_resolved_incident(&harness).await;

    let (status, drafted) = harness
        .member(Method::POST, &postmortem(MINE, &incident), None)
        .await;

    assert_eq!(status, StatusCode::CREATED, "{drafted}");
    let body = drafted["body"].as_str().expect("a body");
    assert!(body.starts_with("# Postmortem: Checkout down\n"), "{body}");
    assert!(
        body.contains(
            "- Window: 2023-11-14 22:13:20 UTC \u{2192} 2023-11-14 22:13:20 UTC (0 minutes)"
        ),
        "{body}"
    );
    for told in [
        "opened \"Checkout down\" as sev3",
        "commented: \"rolled back the deploy\"",
        "attached trace \"a failing payment\"",
        "status investigating \u{2192} resolved",
    ] {
        assert!(body.contains(told), "{told} in {body}");
    }
    assert!(body.contains(&format!("(/traces/{TRACE})")));
    assert!(body.contains("_Write what happened in two sentences._"));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_postmortem_is_drafted_read_edited_and_published_in_the_documented_shape() {
    let harness = Harness::new();
    let incident = a_resolved_incident(&harness).await;
    let path = postmortem(MINE, &incident);

    let (missing_status, missing) = harness.member(Method::GET, &path, None).await;
    let (drafted_status, drafted) = harness.member(Method::POST, &path, None).await;
    let (read_status, read) = harness.member(Method::GET, &path, None).await;
    let (edited_status, edited) = harness
        .member(Method::PATCH, &path, Some(json!({"body": "# Our words\n"})))
        .await;
    let (published_status, published) = harness
        .admin(Method::POST, &format!("{path}/publish"), None)
        .await;
    let (unpublished_status, unpublished) = harness
        .admin(Method::POST, &format!("{path}/unpublish"), None)
        .await;
    let (listed_status, listed) = harness
        .member(
            Method::GET,
            &format!("/api/orgs/{}/postmortems", id(MINE)),
            None,
        )
        .await;

    assert_eq!(missing_status, StatusCode::NOT_FOUND, "{missing}");
    assert_eq!(drafted_status, StatusCode::CREATED);
    assert_eq!(
        drafted
            .as_object()
            .map(|object| {
                let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
                keys.sort_unstable();
                keys
            })
            .expect("an object"),
        vec![
            "author",
            "body",
            "created_ms",
            "edited",
            "id",
            "incident",
            "published_ms",
            "status",
            "updated_ms",
        ]
    );
    assert_eq!(drafted["incident"], incident["id"]);
    assert_eq!(drafted["status"], "draft");
    assert_eq!(drafted["edited"], false);
    assert_eq!(drafted["published_ms"], Value::Null);
    assert_eq!(drafted["created_ms"], NOW * 1_000);
    assert_eq!(drafted["author"]["id"], id(ME));
    assert_eq!(drafted["author"]["email"], "person@example.com");
    assert_eq!(read_status, StatusCode::OK);
    assert_eq!(read, drafted);
    assert_eq!(edited_status, StatusCode::OK);
    assert_eq!(edited["body"], "# Our words\n");
    assert_eq!(edited["edited"], true);
    assert_eq!(edited["id"], drafted["id"]);
    assert_eq!(published_status, StatusCode::OK);
    assert_eq!(published["status"], "published");
    assert_eq!(published["published_ms"], NOW * 1_000);
    assert_eq!(unpublished_status, StatusCode::OK);
    assert_eq!(unpublished["status"], "draft");
    assert_eq!(unpublished["published_ms"], Value::Null);
    assert_eq!(listed_status, StatusCode::OK);
    assert_eq!(
        listed,
        json!({"postmortems": [{"incident": incident["id"], "status": "draft"}]})
    );
}

#[tokio::test]
async fn conflicts_and_invalid_bodies_are_refused_with_409_and_422() {
    let harness = Harness::new();
    let incident = a_resolved_incident(&harness).await;
    let path = postmortem(MINE, &incident);
    harness.member(Method::POST, &path, None).await;
    harness
        .member(Method::PATCH, &path, Some(json!({"body": "mine"})))
        .await;

    let (again_status, again) = harness.member(Method::POST, &path, None).await;
    let (member_force_status, _) = harness
        .member(Method::POST, &format!("{path}?force=true"), None)
        .await;
    let (forced_status, forced) = harness
        .admin(Method::POST, &format!("{path}?force=true"), None)
        .await;
    let (empty_status, _) = harness
        .member(Method::PATCH, &path, Some(json!({"body": "  "})))
        .await;
    let (huge_status, _) = harness
        .member(
            Method::PATCH,
            &path,
            Some(json!({"body": "x".repeat(200 * 1024 + 1)})),
        )
        .await;
    let (absent_status, _) = harness.member(Method::PATCH, &path, Some(json!({}))).await;
    harness
        .admin(Method::POST, &format!("{path}/publish"), None)
        .await;
    let (published_edit_status, _) = harness
        .member(Method::PATCH, &path, Some(json!({"body": "late"})))
        .await;
    let (published_draft_status, _) = harness.member(Method::POST, &path, None).await;

    assert_eq!(again_status, StatusCode::CONFLICT);
    assert!(again.as_str().is_some_and(|text| text.contains("force")));
    assert_eq!(member_force_status, StatusCode::FORBIDDEN);
    assert_eq!(forced_status, StatusCode::CREATED);
    assert_eq!(forced["edited"], false);
    assert!(
        forced["body"]
            .as_str()
            .is_some_and(|text| text.starts_with("# Postmortem"))
    );
    assert_eq!(empty_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(huge_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(absent_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(published_edit_status, StatusCode::CONFLICT);
    assert_eq!(published_draft_status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn publishing_is_for_administrators_a_read_key_only_reads_and_a_stranger_finds_nothing() {
    let harness = Harness::new();
    let incident = a_resolved_incident(&harness).await;
    let path = postmortem(MINE, &incident);
    harness.member(Method::POST, &path, None).await;
    let key = |method, uri: &str, body| {
        harness.served(OrgRole::Member, as_key(WHOLE_KEY, method, uri, body))
    };

    let (member_publish, _) = harness
        .member(Method::POST, &format!("{path}/publish"), None)
        .await;
    let (member_unpublish, _) = harness
        .member(Method::POST, &format!("{path}/unpublish"), None)
        .await;
    let (key_read, read) = key(Method::GET, &path, None).await;
    let (key_list, _) = key(
        Method::GET,
        &format!("/api/orgs/{}/postmortems", id(MINE)),
        None,
    )
    .await;
    let (key_draft, _) = key(Method::POST, &path, None).await;
    let (key_edit, _) = key(Method::PATCH, &path, Some(json!({"body": "x"}))).await;
    let (key_publish, _) = key(Method::POST, &format!("{path}/publish"), None).await;
    let (bound_read, _) = harness
        .served(OrgRole::Member, as_key(BOUND_KEY, Method::GET, &path, None))
        .await;
    let (stranger_read, _) = harness
        .member(Method::GET, &postmortem(THEIRS, &incident), None)
        .await;
    let (stranger_draft, _) = harness
        .member(Method::POST, &postmortem(THEIRS, &incident), None)
        .await;
    let (unknown, _) = harness
        .member(
            Method::POST,
            &format!("{}/{}/postmortem", incidents(MINE), id(999)),
            None,
        )
        .await;
    let anonymous = harness
        .served(
            OrgRole::Member,
            Request::builder()
                .method(Method::GET)
                .uri(&path)
                .header("x-requested-with", "fetch")
                .body(Body::empty())
                .expect("a request"),
        )
        .await;

    assert_eq!(member_publish, StatusCode::FORBIDDEN);
    assert_eq!(member_unpublish, StatusCode::FORBIDDEN);
    assert_eq!(key_read, StatusCode::OK);
    assert_eq!(read["status"], "draft");
    assert_eq!(key_list, StatusCode::OK);
    assert_eq!(key_draft, StatusCode::FORBIDDEN);
    assert_eq!(key_edit, StatusCode::FORBIDDEN);
    assert_eq!(key_publish, StatusCode::FORBIDDEN);
    assert_eq!(
        bound_read,
        StatusCode::NOT_FOUND,
        "no project on this incident"
    );
    assert_eq!(stranger_read, StatusCode::NOT_FOUND);
    assert_eq!(stranger_draft, StatusCode::NOT_FOUND);
    assert_eq!(unknown, StatusCode::NOT_FOUND);
    assert_eq!(anonymous.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn spec_l2_8_every_postmortem_write_is_audited() {
    let harness = Harness::new();
    let incident = a_resolved_incident(&harness).await;
    let path = postmortem(MINE, &incident);
    harness.member(Method::POST, &path, None).await;
    harness
        .member(Method::PATCH, &path, Some(json!({"body": "words"})))
        .await;
    harness
        .admin(Method::POST, &format!("{path}/publish"), None)
        .await;
    harness
        .admin(Method::POST, &format!("{path}/unpublish"), None)
        .await;
    harness
        .member(Method::POST, &format!("{path}/publish"), None)
        .await;

    let entries = harness.notebook.entries();
    let actions: Vec<Action> = entries
        .iter()
        .map(|entry| entry.action)
        .filter(|action| {
            matches!(
                action,
                Action::PostmortemDrafted
                    | Action::PostmortemEdited
                    | Action::PostmortemPublished
                    | Action::PostmortemUnpublished
            )
        })
        .collect();
    assert_eq!(
        actions,
        vec![
            Action::PostmortemDrafted,
            Action::PostmortemEdited,
            Action::PostmortemPublished,
            Action::PostmortemUnpublished,
        ]
    );
    assert!(
        entries
            .iter()
            .filter(|entry| entry.target.kind == TargetKind::Postmortem)
            .all(|entry| entry.detail.get("incident") == Some(incident["id"].as_str()))
    );
}
