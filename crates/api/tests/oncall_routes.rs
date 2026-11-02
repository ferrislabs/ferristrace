use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, LOCATION};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, oncall_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{
    AlertLedger, Bench, Couriered, Crew, IncidentLedger, Inert, Notebook, OncallLedger,
    PriceLedger, StatusLedger, Untransacted,
};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, AckClaim, AckKey, Action, Actor, Agent, ApiKey, ApiKeys, ApiSecret, Assigned,
    Created, Day, Handoff, IncidentId, IssuedKey, KeyId, KeyName, KeyScopes, LiveSession,
    Membership, Memberships, NoPhotos, OrgRole, PageSource, Paging, Project, ProjectId,
    ProjectName, Projects, ProvisionPolicy, ReadGrant, Renamed, Rotation, RotationLength, Schedule,
    ScheduleId, ScheduleName, Session, SessionError, SessionId, SessionRoll, SessionStore,
    SessionToken, StoreError, TargetKind, TeamId, TenantId, TimeOfDay, Timezone, UnixSeconds,
    Weekday, WhoamiError,
};
use ferristrace_infra::FixedClock;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const ME: u128 = 9;
const ADA: u128 = 10;
const BOB: u128 = 11;
const STRANGER: u128 = 12;
const TEAM: u128 = 300;
const SCHEDULE: u128 = 400;
const NOW: u64 = 1_793_613_600;
const WHOLE_KEY: &str = "ft_live_whole.secret";

fn org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

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
                account: account(ME),
                email: "me@example.com".try_into().expect("an address"),
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

struct OneKey;

impl ApiKeys for OneKey {
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
        Ok((secret.exposed() == WHOLE_KEY).then(|| ReadGrant::whole(org(MINE))))
    }
}

struct NoProject;

impl Projects for NoProject {
    async fn of(&self, _tenant: TenantId) -> Result<Vec<Project>, StoreError> {
        Ok(Vec::new())
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
    harness: Shared,
}

#[derive(Clone)]
struct Shared {
    alerts: AlertLedger,
    incidents: IncidentLedger,
    oncall: OncallLedger,
    crew: Crew,
    bench: Bench,
    notebook: Notebook,
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
repo!(ApiKeys, OneKey, |fakes| {
    let _ = fakes;
    OneKey
});
repo!(Projects, NoProject, |fakes| {
    let _ = fakes;
    NoProject
});
repo!(Audit, Notebook, |fakes| fakes.harness.notebook.clone());
repo!(Roster, Crew, |fakes| fakes.harness.crew.clone());
repo!(Teams, Bench, |fakes| fakes.harness.bench.clone());
repo!(AlertRules, AlertLedger, |fakes| fakes
    .harness
    .alerts
    .clone());
repo!(AlertStates, AlertLedger, |fakes| fakes
    .harness
    .alerts
    .clone());
repo!(AlertEvents, AlertLedger, |fakes| fakes
    .harness
    .alerts
    .clone());
repo!(AlertOutbox, AlertLedger, |fakes| fakes
    .harness
    .alerts
    .clone());
repo!(AlertChannels, AlertLedger, |fakes| fakes
    .harness
    .alerts
    .clone());
repo!(AlertDuty, AlertLedger, |fakes| fakes.harness.alerts.clone());
repo!(ModelPrices, PriceLedger, |fakes| {
    let _ = fakes;
    PriceLedger::default()
});
repo!(Incidents, IncidentLedger, |fakes| fakes
    .harness
    .incidents
    .clone());
repo!(IncidentTimeline, IncidentLedger, |fakes| fakes
    .harness
    .incidents
    .clone());
repo!(IncidentAttachments, IncidentLedger, |fakes| fakes
    .harness
    .incidents
    .clone());
repo!(Schedules, OncallLedger, |fakes| fakes
    .harness
    .oncall
    .clone());
repo!(Pages, OncallLedger, |fakes| fakes.harness.oncall.clone());
repo!(StatusOutbox, StatusLedger, |fakes| {
    let _ = fakes;
    StatusLedger::default()
});

struct TestStack;

impl Stack for TestStack {
    type Repos = Fakes;
    type Clock = FixedClock;
    type Hasher = Inert;
    type Mailer = Couriered;
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

fn ack_key() -> AckKey {
    AckKey::derive(&[4_u8; 32])
}

fn weekly() -> Schedule {
    Schedule {
        id: ScheduleId::new(Uuid::from_u128(SCHEDULE)),
        org: org(MINE),
        team: TeamId::new(Uuid::from_u128(TEAM)),
        name: ScheduleName::try_from("Primary").expect("a name"),
        timezone: Timezone::utc(),
        rotation: Rotation::new(vec![account(ME), account(ADA), account(BOB)]).expect("a rotation"),
        handoff: Handoff {
            weekday: Weekday::Monday,
            time: TimeOfDay::try_from("09:00").expect("a time"),
        },
        length: RotationLength::try_from(7).expect("a length"),
        starts_on: Day::try_from("2026-11-02").expect("a day"),
    }
}

struct Harness(Shared);

impl Harness {
    fn new() -> Self {
        let crew = Crew::default();
        crew.seat(org(MINE), account(ME), "me@example.com");
        crew.seat(org(MINE), account(ADA), "ada@example.com");
        crew.seat(org(MINE), account(BOB), "bob@example.com");
        crew.seat(org(THEIRS), account(STRANGER), "stranger@example.com");
        let bench = Bench::default();
        bench.team(
            org(MINE),
            TeamId::new(Uuid::from_u128(TEAM)),
            "Payments",
            &[account(ME), account(ADA), account(BOB)],
        );
        let oncall = OncallLedger::default();
        oncall.put(weekly());
        Self(Shared {
            alerts: AlertLedger::default(),
            incidents: IncidentLedger::default(),
            oncall,
            crew,
            bench,
            notebook: Notebook::default(),
        })
    }

    fn app(&self, role: OrgRole) -> Router {
        let adapters = Adapters {
            clock: FixedClock::at(NOW + 3_600),
            hasher: Inert::default(),
            mailer: Couriered::default(),
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
        oncall_router(AppState::new(
            Arc::new(
                Ferristrace::<TestStack>::new(
                    Fakes {
                        role,
                        harness: self.0.clone(),
                    },
                    adapters,
                )
                .with_ack_key(Some(ack_key())),
            ),
            SessionCookie::default(),
        ))
    }

    async fn served(&self, role: OrgRole, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.app(role).oneshot(request).await.expect("a response");
        let status = response.status();
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let value = match location {
            Some(location) => json!({ "location": location }),
            None => serde_json::from_slice(&body)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned())),
        };
        (status, value)
    }

    async fn as_role(
        &self,
        role: OrgRole,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.served(role, as_session(method, uri, body)).await
    }

    async fn page(&self, target_week: u64) -> (IncidentId, ferristrace_domain::Page) {
        let incident = IncidentId::fresh();
        let page = Paging::new(self.0.oncall.clone(), self.0.oncall.clone())
            .raise(
                org(MINE),
                weekly().id,
                PageSource::Incident(incident),
                "Checkout down",
                (NOW + target_week * 7 * 86_400 + 60) * 1_000,
            )
            .await
            .expect("asked")
            .expect("raised");
        (incident, page)
    }

    fn actions(&self) -> Vec<Action> {
        self.0
            .notebook
            .entries()
            .into_iter()
            .map(|entry| entry.action)
            .collect()
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

fn as_key(method: Method, uri: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-requested-with", "fetch")
        .header(AUTHORIZATION, format!("Bearer {WHOLE_KEY}"));
    match body {
        Some(body) => builder
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("a request")
}

fn anonymous(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("a request")
}

fn team_schedules() -> String {
    format!("/api/orgs/{}/teams/{}/schedules", id(MINE), id(TEAM))
}

fn schedule(seed: u128) -> String {
    format!("/api/orgs/{}/schedules/{}", id(MINE), id(seed))
}

fn a_draft() -> Value {
    json!({
        "name": "Secondary",
        "timezone": "Europe/Paris",
        "rotation": [id(BOB), id(ADA)],
        "handoff": {"weekday": "wednesday", "time": "10:30"},
        "length_days": 14,
        "starts_on": "2026-11-04",
    })
}

#[tokio::test]
async fn an_administrator_creates_reads_changes_and_deletes_a_schedule() {
    let harness = Harness::new();

    let (created, made) = harness
        .as_role(
            OrgRole::Admin,
            Method::POST,
            &team_schedules(),
            Some(a_draft()),
        )
        .await;
    let made_id = made["id"].as_str().expect("an id").to_owned();
    let (listed, list) = harness
        .as_role(OrgRole::Member, Method::GET, &team_schedules(), None)
        .await;
    let (patched, changed) = harness
        .as_role(
            OrgRole::Admin,
            Method::PATCH,
            &format!("/api/orgs/{}/schedules/{made_id}", id(MINE)),
            Some(json!({"rotation": [id(ADA)], "handoff": {"weekday": "friday", "time": "08:00"}})),
        )
        .await;
    let (deleted, _) = harness
        .as_role(
            OrgRole::Admin,
            Method::DELETE,
            &format!("/api/orgs/{}/schedules/{made_id}", id(MINE)),
            None,
        )
        .await;

    assert_eq!(created, StatusCode::CREATED, "{made}");
    assert_eq!(
        made,
        json!({
            "id": made_id,
            "team": id(TEAM),
            "name": "Secondary",
            "timezone": "Europe/Paris",
            "rotation": [id(BOB), id(ADA)],
            "handoff": {"weekday": "wednesday", "time": "10:30"},
            "length_days": 14,
            "starts_on": "2026-11-04",
            "overrides": [],
        })
    );
    assert_eq!(listed, StatusCode::OK);
    assert_eq!(list["schedules"].as_array().map(Vec::len), Some(2));
    assert_eq!(patched, StatusCode::OK, "{changed}");
    assert_eq!(changed["rotation"], json!([id(ADA)]));
    assert_eq!(
        changed["handoff"],
        json!({"weekday": "friday", "time": "08:00"})
    );
    assert_eq!(deleted, StatusCode::NO_CONTENT);
    assert_eq!(
        harness.actions(),
        vec![
            Action::ScheduleCreated,
            Action::ScheduleUpdated,
            Action::ScheduleDeleted
        ]
    );
}

#[tokio::test]
async fn members_and_read_keys_read_schedules_but_never_write_them() {
    let harness = Harness::new();

    let (member, _) = harness
        .as_role(
            OrgRole::Member,
            Method::POST,
            &team_schedules(),
            Some(a_draft()),
        )
        .await;
    let (read, one) = harness
        .served(
            OrgRole::Member,
            as_key(Method::GET, &schedule(SCHEDULE), None),
        )
        .await;
    let (keyed_write, _) = harness
        .served(
            OrgRole::Owner,
            as_key(Method::POST, &team_schedules(), Some(a_draft())),
        )
        .await;
    let (keyed_delete, _) = harness
        .served(
            OrgRole::Owner,
            as_key(Method::DELETE, &schedule(SCHEDULE), None),
        )
        .await;

    assert_eq!(member, StatusCode::FORBIDDEN);
    assert_eq!(read, StatusCode::OK, "{one}");
    assert_eq!(one["name"], "Primary");
    assert_eq!(keyed_write, StatusCode::UNAUTHORIZED);
    assert_eq!(keyed_delete, StatusCode::UNAUTHORIZED);
    assert_eq!(harness.0.oncall.schedules().len(), 1);
    assert!(harness.actions().is_empty());
}

#[tokio::test]
async fn a_schedule_is_refused_when_misshapen_or_staffed_from_outside_the_team() {
    let harness = Harness::new();
    let mut bad_zone = a_draft();
    bad_zone["timezone"] = json!("Mars/Olympus");
    let mut bad_time = a_draft();
    bad_time["handoff"]["time"] = json!("25:00");
    let mut too_long = a_draft();
    too_long["length_days"] = json!(29);
    let mut outsider = a_draft();
    outsider["rotation"] = json!([id(STRANGER)]);

    for body in [bad_zone, bad_time, too_long, outsider] {
        let (status, message) = harness
            .as_role(OrgRole::Admin, Method::POST, &team_schedules(), Some(body))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{message}");
    }
    let (elsewhere, _) = harness
        .as_role(
            OrgRole::Admin,
            Method::GET,
            &format!("/api/orgs/{}/teams/{}/schedules", id(MINE), id(999)),
            None,
        )
        .await;
    let (foreign, _) = harness
        .as_role(
            OrgRole::Admin,
            Method::GET,
            &format!("/api/orgs/{}/schedules/{}", id(THEIRS), id(SCHEDULE)),
            None,
        )
        .await;

    assert_eq!(elsewhere, StatusCode::NOT_FOUND);
    assert_eq!(foreign, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_override_changes_who_is_on_call_now_and_is_audited() {
    let harness = Harness::new();
    let now = format!("{}/now", schedule(SCHEDULE));

    let (before, rotation) = harness
        .as_role(OrgRole::Member, Method::GET, &now, None)
        .await;
    let (created, cover) = harness
        .as_role(
            OrgRole::Admin,
            Method::POST,
            &format!("{}/overrides", schedule(SCHEDULE)),
            Some(
                json!({"account": id(BOB), "from_ms": NOW * 1_000, "to_ms": (NOW + 7_200) * 1_000}),
            ),
        )
        .await;
    let (_, covered) = harness
        .as_role(OrgRole::Member, Method::GET, &now, None)
        .await;
    let cover_id = cover["id"].as_str().expect("an id").to_owned();
    let (removed, _) = harness
        .as_role(
            OrgRole::Admin,
            Method::DELETE,
            &format!("{}/overrides/{cover_id}", schedule(SCHEDULE)),
            None,
        )
        .await;
    let (stranger, _) = harness
        .as_role(
            OrgRole::Admin,
            Method::POST,
            &format!("{}/overrides", schedule(SCHEDULE)),
            Some(json!({"account": id(STRANGER), "from_ms": 1, "to_ms": 2})),
        )
        .await;

    assert_eq!(before, StatusCode::OK);
    assert_eq!(
        rotation,
        json!({
            "account": {"id": id(ME), "email": "me@example.com", "name": null},
            "until_ms": (NOW + 7 * 86_400 - 3_600) * 1_000,
        })
    );
    assert_eq!(created, StatusCode::CREATED, "{cover}");
    assert_eq!(cover["account"], json!(id(BOB)));
    assert_eq!(covered["account"]["id"], json!(id(BOB)));
    assert_eq!(covered["until_ms"], json!((NOW + 7_200) * 1_000));
    assert_eq!(removed, StatusCode::NO_CONTENT);
    assert_eq!(stranger, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        harness.actions(),
        vec![Action::OverrideCreated, Action::OverrideDeleted]
    );
}

#[tokio::test]
async fn open_pages_list_with_their_target_and_source() {
    let harness = Harness::new();
    let (incident, page) = harness.page(0).await;

    let (status, listed) = harness
        .as_role(
            OrgRole::Member,
            Method::GET,
            &format!("/api/orgs/{}/pages?state=open", id(MINE)),
            None,
        )
        .await;
    let (keyed, _) = harness
        .served(
            OrgRole::Member,
            as_key(Method::GET, &format!("/api/orgs/{}/pages", id(MINE)), None),
        )
        .await;
    let (bad, _) = harness
        .as_role(
            OrgRole::Member,
            Method::GET,
            &format!("/api/orgs/{}/pages?state=sleeping", id(MINE)),
            None,
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listed,
        json!({"pages": [{
            "id": page.id.to_string(),
            "schedule": {"id": id(SCHEDULE), "name": "Primary"},
            "target": {"id": id(ME), "email": "me@example.com", "name": null},
            "source": {"kind": "incident", "incident": incident.to_string(), "title": "Checkout down"},
            "headline": "Checkout down",
            "state": "triggered",
            "step": 0,
            "created_ms": (NOW + 60) * 1_000,
            "escalate_at_ms": (NOW + 60 + 900) * 1_000,
            "acknowledged_by": null,
            "acknowledged_ms": null,
            "resolved_ms": null,
        }]})
    );
    assert_eq!(keyed, StatusCode::OK);
    assert_eq!(bad, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_target_or_an_administrator_acknowledges_in_the_app_and_a_key_never_does() {
    let harness = Harness::new();
    let (_, mine) = harness.page(0).await;
    let (_, adas) = harness.page(1).await;
    let ack =
        |page: &ferristrace_domain::Page| format!("/api/orgs/{}/pages/{}/ack", id(MINE), page.id);

    let (keyed, _) = harness
        .served(OrgRole::Owner, as_key(Method::POST, &ack(&mine), None))
        .await;
    let (own, acknowledged) = harness
        .as_role(OrgRole::Member, Method::POST, &ack(&mine), None)
        .await;
    let (twice, _) = harness
        .as_role(OrgRole::Member, Method::POST, &ack(&mine), None)
        .await;
    let (others, _) = harness
        .as_role(OrgRole::Member, Method::POST, &ack(&adas), None)
        .await;
    let (admin, _) = harness
        .as_role(OrgRole::Admin, Method::POST, &ack(&adas), None)
        .await;

    assert_eq!(keyed, StatusCode::FORBIDDEN);
    assert_eq!(own, StatusCode::OK, "{acknowledged}");
    assert_eq!(acknowledged["state"], "acknowledged");
    assert_eq!(acknowledged["acknowledged_by"], json!(id(ME)));
    assert_eq!(twice, StatusCode::OK, "acknowledging twice is fine");
    assert_eq!(others, StatusCode::FORBIDDEN);
    assert_eq!(admin, StatusCode::OK);
    let logged = harness.0.notebook.entries();
    assert_eq!(logged.len(), 2, "one entry per acknowledged page");
    assert!(
        logged
            .iter()
            .all(|entry| entry.action == Action::PageAcknowledged
                && entry.actor == Actor::Account(account(ME))
                && entry.target.kind == TargetKind::Page)
    );
}

fn link(page: &ferristrace_domain::Page, key: &AckKey, expires: u64) -> String {
    let token = key.sign(
        &AckClaim {
            org: page.org,
            page: page.id,
            account: page.target,
        },
        UnixSeconds::new(expires),
    );
    format!("/api/pages/ack?token={token}")
}

#[tokio::test]
async fn a_signed_link_acknowledges_without_a_session_and_lands_on_the_incident() {
    let harness = Harness::new();
    let (incident, page) = harness.page(0).await;
    let uri = link(&page, &ack_key(), NOW + 86_400);

    let (status, landed) = harness.served(OrgRole::Member, anonymous(&uri)).await;
    let (twice, again) = harness.served(OrgRole::Member, anonymous(&uri)).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        landed,
        json!({"location": format!("/incidents/{incident}")})
    );
    assert_eq!(twice, StatusCode::SEE_OTHER, "a second click is harmless");
    assert_eq!(again, landed);
    let held = &harness.0.oncall.pages()[0];
    assert_eq!(held.state.as_str(), "acknowledged");
    assert_eq!(held.acknowledged_by, Some(account(ME)));
    let logged = harness.0.notebook.entries();
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].detail.get("via"), Some(Some("link")));
    let told = harness.0.incidents.timeline();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].incident, incident);
}

#[tokio::test]
async fn a_tampered_forged_or_expired_link_acknowledges_nothing() {
    let harness = Harness::new();
    let (_, page) = harness.page(0).await;
    let valid = link(&page, &ack_key(), NOW + 86_400);
    let tampered = valid.replace(
        &page.id.as_uuid().simple().to_string(),
        &Uuid::from_u128(77).simple().to_string(),
    );
    let forged = link(&page, &AckKey::derive(&[9_u8; 32]), NOW + 86_400);
    let expired = link(&page, &ack_key(), NOW + 60);

    let (changed, _) = harness.served(OrgRole::Member, anonymous(&tampered)).await;
    let (foreign, _) = harness.served(OrgRole::Member, anonymous(&forged)).await;
    let (late, _) = harness.served(OrgRole::Member, anonymous(&expired)).await;
    let (garbage, _) = harness
        .served(OrgRole::Member, anonymous("/api/pages/ack?token=nonsense"))
        .await;
    let (missing, _) = harness
        .served(OrgRole::Member, anonymous("/api/pages/ack"))
        .await;

    assert_eq!(changed, StatusCode::FORBIDDEN);
    assert_eq!(foreign, StatusCode::FORBIDDEN);
    assert_eq!(late, StatusCode::GONE);
    assert_eq!(garbage, StatusCode::FORBIDDEN);
    assert_eq!(missing, StatusCode::FORBIDDEN);
    assert_eq!(harness.0.oncall.pages()[0].state.as_str(), "triggered");
    assert!(harness.0.notebook.entries().is_empty());
}

#[tokio::test]
async fn an_alert_page_link_lands_on_the_alert() {
    let harness = Harness::new();
    let rule = ferristrace_domain::RuleId::fresh();
    let page = Paging::new(harness.0.oncall.clone(), harness.0.oncall.clone())
        .raise(
            org(MINE),
            weekly().id,
            PageSource::Alert {
                rule,
                event: ferristrace_domain::EventId::fresh(),
            },
            "Checkout errors",
            (NOW + 60) * 1_000,
        )
        .await
        .expect("asked")
        .expect("raised");

    let (status, landed) = harness
        .served(
            OrgRole::Member,
            anonymous(&link(&page, &ack_key(), NOW + 86_400)),
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(landed, json!({"location": format!("/alerts?rule={rule}")}));
    assert!(harness.0.incidents.timeline().is_empty());
}

#[derive(Clone, Default)]
struct Moving(Arc<std::sync::atomic::AtomicU64>);

impl Moving {
    fn to(&self, second: u64) {
        self.0.store(second, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ferristrace_domain::Clock for Moving {
    fn now(&self) -> UnixSeconds {
        UnixSeconds::new(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}

struct Failing;

impl ferristrace_domain::PlacementReader for Failing {
    async fn placements(
        &self,
        _tenant: TenantId,
        _window: ferristrace_domain::Window,
        _project: Option<ProjectId>,
    ) -> Result<ferristrace_domain::Placements, StoreError> {
        Ok(ferristrace_domain::Placements::default())
    }
}

impl ferristrace_domain::CulpritReader for Failing {
    async fn culprits(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _signal: &ferristrace_domain::Signal,
        _window: ferristrace_domain::Window,
    ) -> Result<ferristrace_domain::Culprits, StoreError> {
        Ok(ferristrace_domain::Culprits::default())
    }
}

impl ferristrace_domain::SpanReader for Failing {
    async fn services(
        &self,
        _tenant: TenantId,
        _filter: &ferristrace_domain::ServiceFilter,
    ) -> Result<Vec<ferristrace_domain::ServiceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn operations(
        &self,
        _tenant: TenantId,
        _service: &str,
        _filter: &ferristrace_domain::ServiceFilter,
    ) -> Result<Vec<ferristrace_domain::ServiceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn series(
        &self,
        _tenant: TenantId,
        _service: Option<&str>,
        _filter: &ferristrace_domain::ServiceFilter,
    ) -> Result<ferristrace_domain::ServiceSeries, StoreError> {
        Ok(ferristrace_domain::ServiceSeries {
            requests: 100,
            errors: 40,
            ..ferristrace_domain::ServiceSeries::default()
        })
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        _filter: &ferristrace_domain::TraceFilter,
    ) -> Result<Vec<ferristrace_domain::TraceSummary>, StoreError> {
        Ok(Vec::new())
    }

    async fn trace(
        &self,
        _tenant: TenantId,
        _id: ferristrace_domain::TraceId,
        _project: Option<ProjectId>,
    ) -> Result<Vec<ferristrace_domain::Span>, StoreError> {
        Ok(Vec::new())
    }
}

impl ferristrace_domain::SpanPages for Failing {
    async fn page(
        &self,
        _org: TenantId,
        _after: Option<&ferristrace_domain::SpanCursor>,
        _limit: u32,
    ) -> Result<ferristrace_domain::SpanPage, StoreError> {
        Ok(ferristrace_domain::SpanPage::default())
    }
}

struct Watching;

impl Stack for Watching {
    type Repos = Fakes;
    type Clock = Moving;
    type Hasher = Inert;
    type Mailer = Couriered;
    type Photos = NoPhotos;
    type SpanStore = Failing;
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

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn spec_l2_5_a_firing_alert_pages_escalates_and_stops_once_acknowledged() {
    let harness = Harness::new();
    for (seed, email) in [
        (ME, "me@example.com"),
        (ADA, "ada@example.com"),
        (BOB, "bob@example.com"),
    ] {
        harness.0.oncall.address(account(seed), email);
    }
    ferristrace_domain::AlertRules::create(
        &harness.0.alerts,
        org(MINE),
        &ferristrace_domain::RuleDraft {
            name: ferristrace_domain::RuleName::try_from("Checkout errors").expect("a name"),
            project: None,
            enabled: true,
            definition: ferristrace_domain::Definition::new(ferristrace_domain::DefinitionParts {
                signal: ferristrace_domain::Signal::Traces(ferristrace_domain::TraceSignal {
                    service: Some("checkout".into()),
                    operation: None,
                    environment: None,
                    measure: ferristrace_domain::TraceMeasure::ErrorRate,
                    placement: ferristrace_domain::Placement::default(),
                }),
                condition: ferristrace_domain::Condition::new(
                    ferristrace_domain::Comparison::Above,
                    5.0,
                )
                .expect("a threshold"),
                window_secs: 300,
                pending_secs: 0,
                severity: ferristrace_domain::AlertSeverity::Critical,
                no_data: ferristrace_domain::NoDataPolicy::Ok,
                channels: Vec::new(),
                page: Some(weekly().id),
            })
            .expect("a definition"),
        },
        account(ME),
    )
    .await
    .expect("kept")
    .expect("fresh");
    let clock = Moving::default();
    let mail = Couriered::default();
    let app = Ferristrace::<Watching>::new(
        Fakes {
            role: OrgRole::Admin,
            harness: harness.0.clone(),
        },
        Adapters {
            clock: clock.clone(),
            hasher: Inert::default(),
            mailer: mail.clone(),
            webhooks: Inert::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: Some(Failing),
            issue_store: None,
            log_store: None,
            metric_store: None,
            oidc: None,
            provision: ProvisionPolicy::default(),
        },
    )
    .with_ack_key(Some(ack_key()));
    let fired_at = NOW + 7 * 86_400 + 3_600;

    clock.to(fired_at);
    app.evaluate_alerts().await.expect("evaluated");
    app.escalate_pages().await.expect("a page tick");
    clock.to(fired_at + 899);
    let early = app.escalate_pages().await.expect("a page tick");
    clock.to(fired_at + 900);
    app.escalate_pages().await.expect("a page tick");
    let page = harness.0.oncall.pages()[0].clone();
    app.acknowledge_page(
        &ferristrace_domain::Reader::Session(SessionToken::from_plaintext(TOKEN)),
        org(MINE),
        page.id,
    )
    .await
    .expect("acknowledged");
    for later in [1_800, 2_700, 3_600, 9_000] {
        clock.to(fired_at + later);
        app.escalate_pages().await.expect("a page tick");
    }

    assert_eq!(page.target, account(BOB), "the second person of week two");
    assert_eq!(page.step, 1);
    assert!(matches!(
        early,
        ferristrace_domain::Paged::Worked {
            escalated: 0,
            sent: 0,
            ..
        }
    ));
    let mailed: Vec<(String, u32, bool)> = mail
        .paged()
        .into_iter()
        .map(|(to, dispatch)| (to.to_string(), dispatch.step, dispatch.ack.is_some()))
        .collect();
    assert_eq!(
        mailed,
        vec![
            ("ada@example.com".to_owned(), 0, true),
            ("bob@example.com".to_owned(), 1, true),
        ],
        "one email per step, each with its acknowledgement link, and none after the acknowledgement"
    );
    assert_eq!(harness.0.oncall.pages()[0].state.as_str(), "acknowledged");
    let escalations = harness
        .actions()
        .into_iter()
        .filter(|action| *action == Action::PageEscalated)
        .count();
    assert_eq!(escalations, 1);
}
