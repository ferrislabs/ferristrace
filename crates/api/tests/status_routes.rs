use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, incident_router, status_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{
    AlertLedger, Couriered, Crew, IncidentLedger, Inert, Notebook, OncallLedger, PostmortemLedger,
    StatusLedger, Untransacted,
};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Action, Agent, ApiKey, ApiKeys, ApiSecret, Assigned, Created, IssuedKey, KeyId,
    KeyName, KeyScopes, LiveSession, Membership, Memberships, NoPhotos, OrgRole, Project,
    ProjectId, ProjectName, Projects, ProvisionPolicy, ReadGrant, Renamed, Session, SessionError,
    SessionId, SessionRoll, SessionStore, SessionToken, StoreError, TargetKind, TeamId, TenantId,
    UnixSeconds, WhoamiError,
};
use ferristrace_domain::{
    AlertRules, AlertSeverity, AlertStates, AlertStatus, Comparison, Condition, Definition,
    DefinitionParts, IncidentId, Labels, LinkClaim, LinkKey, NoDataPolicy, Postmortem,
    PostmortemBody, PostmortemId, PostmortemStatus, Postmortems, Purpose, RuleDraft, RuleId,
    RuleName, RuleState, SeriesState, Signal, StatusPageId, SubscriberId, TraceMeasure,
    TraceSignal,
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
const LINK_SECRET: [u8; 32] = [7; 32];

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
    status: StatusLedger,
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
repo!(Postmortems, PostmortemLedger, |fakes| fakes
    .postmortems
    .clone());
repo!(StatusPages, StatusLedger, |fakes| fakes.status.clone());
repo!(StatusSubscribers, StatusLedger, |fakes| fakes
    .status
    .clone());
repo!(StatusOutbox, StatusLedger, |fakes| fakes.status.clone());

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

struct Harness {
    alerts: AlertLedger,
    incidents: IncidentLedger,
    crew: Crew,
    notebook: Notebook,
    oncall: OncallLedger,
    postmortems: PostmortemLedger,
    status: StatusLedger,
    courier: Couriered,
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
            status: StatusLedger::default(),
            courier: Couriered::default(),
        }
    }

    fn state_app(&self) -> Arc<Ferristrace<TestStack>> {
        self.ferristrace(OrgRole::Member)
    }

    fn app(&self, role: OrgRole) -> Router {
        let state = AppState::new(self.ferristrace(role), SessionCookie::default());
        incident_router(state.clone()).merge(status_router(state))
    }

    fn ferristrace(&self, role: OrgRole) -> Arc<Ferristrace<TestStack>> {
        let adapters = Adapters {
            clock: FixedClock::at(NOW),
            hasher: Inert::default(),
            mailer: self.courier.clone(),
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
        Arc::new(
            Ferristrace::<TestStack>::new(
                Fakes {
                    role,
                    alerts: self.alerts.clone(),
                    incidents: self.incidents.clone(),
                    crew: self.crew.clone(),
                    notebook: self.notebook.clone(),
                    oncall: self.oncall.clone(),
                    postmortems: self.postmortems.clone(),
                    status: self.status.clone(),
                },
                adapters,
            )
            .with_status_key(Some(LinkKey::derive(&LINK_SECRET))),
        )
    }

    async fn raw(&self, role: OrgRole, request: Request<Body>) -> axum::response::Response {
        self.app(role).oneshot(request).await.expect("a response")
    }

    async fn served(&self, role: OrgRole, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.raw(role, request).await;
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

    async fn public(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.served(OrgRole::Member, bare(method, uri, body)).await
    }

    async fn a_page(&self, slug: &str, public: bool) -> Value {
        let (status, page) = self
            .admin(
                Method::POST,
                &pages(MINE),
                Some(json!({"name": "Acme", "slug": slug, "public": public})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page
    }

    async fn a_component(&self, page: &Value, body: Value) -> Value {
        let (status, component) = self
            .admin(Method::POST, &components(page), Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{component}");
        component
    }

    async fn declared(&self, body: Value) -> Value {
        let (status, incident) = self
            .member(Method::POST, &incidents(MINE), Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{incident}");
        incident
    }

    async fn a_firing_rule(&self, severity: AlertSeverity) -> RuleId {
        let rule = AlertRules::create(
            &self.alerts,
            org(MINE),
            &RuleDraft {
                name: RuleName::try_from("Payments errors").expect("a name"),
                project: None,
                enabled: true,
                definition: Definition::new(DefinitionParts {
                    signal: Signal::Traces(TraceSignal {
                        service: Some("payments".into()),
                        operation: None,
                        environment: None,
                        measure: TraceMeasure::ErrorRate,
                        placement: ferristrace_domain::Placement::default(),
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity,
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
                    status: AlertStatus::Firing,
                    value: Some(30.0),
                    since_ms: 1,
                    evaluated_ms: 1,
                },
            },
        )
        .await
        .expect("kept");
        rule.id
    }

    async fn subscribed(&self, slug: &str, email: &str) -> (StatusPageId, SubscriberId) {
        let (status, _) = self
            .public(
                Method::POST,
                &format!("/api/status/{slug}/subscribe"),
                Some(json!({"email": email})),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let held = self
            .status
            .subscribers()
            .into_iter()
            .find(|subscriber| subscriber.email.as_str() == email)
            .expect("kept");
        (held.page, held.id)
    }

    async fn confirmed(&self, slug: &str, email: &str) {
        let (page, subscriber) = self.subscribed(slug, email).await;
        let response = self
            .raw(
                OrgRole::Member,
                bare(
                    Method::GET,
                    &format!(
                        "/api/status/confirm?token={}",
                        signed(Purpose::Confirm, page, subscriber, NOW + 60)
                    ),
                    None,
                ),
            )
            .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
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

fn bare(method: Method, uri: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder().method(method).uri(uri);
    match body {
        Some(body) => builder
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("a request")
}

fn signed(purpose: Purpose, page: StatusPageId, subscriber: SubscriberId, expires: u64) -> String {
    LinkKey::derive(&LINK_SECRET)
        .sign(
            &LinkClaim {
                purpose,
                page,
                subscriber,
            },
            UnixSeconds::new(expires),
        )
        .to_string()
}

fn pages(org: u128) -> String {
    format!("/api/orgs/{}/status-pages", id(org))
}

fn page_at(page: &Value) -> String {
    format!("{}/{}", pages(MINE), page["id"].as_str().expect("an id"))
}

fn components(page: &Value) -> String {
    format!("{}/components", page_at(page))
}

fn incidents(org: u128) -> String {
    format!("/api/orgs/{}/incidents", id(org))
}

fn incident_at(incident: &Value) -> String {
    format!(
        "{}/{}",
        incidents(MINE),
        incident["id"].as_str().expect("an id")
    )
}

#[tokio::test]
async fn an_administrator_builds_a_page_and_members_read_it() {
    let harness = Harness::new();

    let (created, page) = harness
        .admin(
            Method::POST,
            &pages(MINE),
            Some(json!({
                "name": "Acme",
                "slug": "acme",
                "public": true,
                "hide_from_search": true,
                "theme": {"accent": "teal", "mode": "dark"},
                "intro": "Live status of Acme"
            })),
        )
        .await;
    let (patched, changed) = harness
        .admin(
            Method::PATCH,
            &page_at(&page),
            Some(json!({"name": "Acme Inc", "intro": null})),
        )
        .await;
    let (listed, list) = harness.member(Method::GET, &pages(MINE), None).await;
    let (read, one) = harness.member(Method::GET, &page_at(&page), None).await;

    assert_eq!(created, StatusCode::CREATED, "{page}");
    assert_eq!(page["slug"], "acme");
    assert_eq!(page["public"], true);
    assert_eq!(page["hide_from_search"], true);
    assert_eq!(page["theme"], json!({"accent": "teal", "mode": "dark"}));
    assert_eq!(page["intro"], "Live status of Acme");
    assert_eq!(page["subscribers"], 0);
    assert_eq!(page["components"], json!([]));
    assert_eq!(patched, StatusCode::OK, "{changed}");
    assert_eq!(changed["name"], "Acme Inc");
    assert_eq!(changed["intro"], Value::Null);
    assert_eq!(listed, StatusCode::OK);
    assert_eq!(list["status_pages"][0]["name"], "Acme Inc");
    assert_eq!(read, StatusCode::OK);
    assert_eq!(one["slug"], "acme");
}

#[tokio::test]
async fn components_are_linked_ordered_changed_and_removed() {
    let harness = Harness::new();
    let rule = harness.a_firing_rule(AlertSeverity::Warning).await;
    let page = harness.a_page("acme", true).await;

    let website = harness
        .a_component(
            &page,
            json!({"name": "Website", "link": {"kind": "manual"}}),
        )
        .await;
    let checkout = harness
        .a_component(
            &page,
            json!({
                "name": "Checkout",
                "link": {"kind": "service", "service": "checkout-api", "project": id(OURS)},
            }),
        )
        .await;
    let payments = harness
        .a_component(
            &page,
            json!({"name": "Payments", "link": {"kind": "alert", "alert": rule.as_uuid()}}),
        )
        .await;
    let (ordered, order) = harness
        .admin(
            Method::POST,
            &format!("{}/order", components(&page)),
            Some(json!({"ids": [payments["id"], checkout["id"], website["id"]]})),
        )
        .await;
    let (changed, website_changed) = harness
        .admin(
            Method::PATCH,
            &format!(
                "{}/{}",
                components(&page),
                website["id"].as_str().expect("an id")
            ),
            Some(json!({"manual_status": "down", "public": false})),
        )
        .await;
    let (removed, _) = harness
        .admin(
            Method::DELETE,
            &format!(
                "{}/{}",
                components(&page),
                checkout["id"].as_str().expect("an id")
            ),
            None,
        )
        .await;
    let (_, sheet) = harness.member(Method::GET, &page_at(&page), None).await;

    assert_eq!(
        checkout["link"],
        json!({"kind": "service", "service": "checkout-api", "project": id(OURS)})
    );
    assert_eq!(
        payments["link"],
        json!({"kind": "alert", "alert": rule.as_uuid()})
    );
    assert_eq!(website["link"], json!({"kind": "manual"}));
    assert_eq!(website["public"], true);
    assert_eq!(ordered, StatusCode::OK, "{order}");
    assert_eq!(order["components"][0]["name"], "Payments");
    assert_eq!(changed, StatusCode::OK, "{website_changed}");
    assert_eq!(website_changed["manual_status"], "down");
    assert_eq!(removed, StatusCode::NO_CONTENT);
    let names: Vec<&str> = sheet["components"]
        .as_array()
        .expect("components")
        .iter()
        .map(|component| component["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, ["Payments", "Website"]);
}

#[tokio::test]
async fn only_administrators_write_and_a_read_key_or_no_session_never_does() {
    let harness = Harness::new();
    let page = harness.a_page("acme", false).await;
    let create = json!({"name": "Other", "slug": "other"});

    let (member, _) = harness
        .member(Method::POST, &pages(MINE), Some(create.clone()))
        .await;
    let (keyed, _) = harness
        .served(
            OrgRole::Admin,
            as_key(WHOLE_KEY, Method::DELETE, &page_at(&page), None),
        )
        .await;
    let (key_read, _) = harness
        .served(
            OrgRole::Admin,
            as_key(WHOLE_KEY, Method::GET, &page_at(&page), None),
        )
        .await;
    let (nobody, _) = harness
        .public(Method::POST, &pages(MINE), Some(create))
        .await;
    let (stranger, _) = harness.admin(Method::GET, &pages(THEIRS), None).await;
    let (member_reads, _) = harness.member(Method::GET, &page_at(&page), None).await;

    assert_eq!(member, StatusCode::FORBIDDEN);
    assert_eq!(keyed, StatusCode::FORBIDDEN);
    assert_eq!(key_read, StatusCode::OK);
    assert_eq!(nobody, StatusCode::FORBIDDEN);
    assert_eq!(stranger, StatusCode::NOT_FOUND);
    assert_eq!(member_reads, StatusCode::OK);
}

#[tokio::test]
async fn a_slug_is_checked_and_unique() {
    let harness = Harness::new();
    harness.a_page("acme", true).await;

    let (taken, _) = harness
        .admin(
            Method::POST,
            &pages(MINE),
            Some(json!({"name": "Again", "slug": "acme"})),
        )
        .await;
    let (bad, _) = harness
        .admin(
            Method::POST,
            &pages(MINE),
            Some(json!({"name": "Bad", "slug": "Not A Slug"})),
        )
        .await;
    let (reserved, _) = harness
        .admin(
            Method::POST,
            &pages(MINE),
            Some(json!({"name": "Bad", "slug": "confirm"})),
        )
        .await;

    assert_eq!(taken, StatusCode::CONFLICT);
    assert_eq!(bad, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reserved, StatusCode::UNPROCESSABLE_ENTITY);
}

async fn a_public_incident_with_secrets(harness: &Harness) -> (RuleId, StatusCode, Value) {
    let rule = harness.a_firing_rule(AlertSeverity::Critical).await;
    let page = harness.a_page("acme", true).await;
    harness
        .a_component(
            &page,
            json!({"name": "Payments", "link": {"kind": "alert", "alert": rule.as_uuid()}}),
        )
        .await;
    harness
        .a_component(
            &page,
            json!({"name": "Admin console", "link": {"kind": "manual"}, "public": false}),
        )
        .await;
    let public = harness
        .declared(json!({"title": "Payments fail", "public": true, "alert": rule.as_uuid()}))
        .await;
    harness.declared(json!({"title": "Secret db outage"})).await;
    let path = incident_at(&public);
    harness
        .member(
            Method::PATCH,
            &path,
            Some(json!({"owner": id(COLLEAGUE), "status": "identified"})),
        )
        .await;
    harness
        .member(
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "db 10.0.0.4 is full, ping colleague@example.com"})),
        )
        .await;
    let (posted, entry) = harness
        .member(
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "Some card payments fail.", "public": true})),
        )
        .await;
    harness
        .member(
            Method::POST,
            &format!("{path}/attachments"),
            Some(json!({"kind": "trace", "reference": TRACE, "label": "the failing payment"})),
        )
        .await;
    (rule, posted, entry)
}

#[tokio::test]
async fn spec_l2_7_the_public_page_needs_no_session_and_shows_only_what_is_public() {
    let harness = Harness::new();
    let (rule, posted, entry) = a_public_incident_with_secrets(&harness).await;

    let response = harness
        .raw(OrgRole::Member, bare(Method::GET, "/api/status/acme", None))
        .await;
    let headers = response.headers().clone();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let shown: Value = serde_json::from_slice(&body).expect("json");

    assert_eq!(posted, StatusCode::CREATED);
    assert_eq!(entry["detail"]["public"], true);
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(
        headers
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("*")
    );
    assert_eq!(
        shown["page"],
        json!({
            "name": "Acme",
            "theme": {"accent": "indigo", "mode": "auto"},
            "intro": null,
            "hide_from_search": false,
        })
    );
    assert_eq!(shown["overall"], "down");
    assert_eq!(
        shown["components"],
        json!([{"name": "Payments", "status": "down"}])
    );
    let incidents = shown["incidents"].as_array().expect("incidents");
    assert_eq!(incidents.len(), 1);
    let incident = &incidents[0];
    assert_eq!(incident["title"], "Payments fail");
    assert_eq!(incident["status"], "identified");
    assert_eq!(incident["severity"], "sev2");
    assert_eq!(incident["resolved_ms"], Value::Null);
    assert_eq!(incident["postmortem_url"], Value::Null);
    let updates: Vec<(&str, &str)> = incident["updates"]
        .as_array()
        .expect("updates")
        .iter()
        .map(|update| {
            (
                update["kind"].as_str().expect("a kind"),
                update["text"].as_str().expect("a text"),
            )
        })
        .collect();
    assert_eq!(
        updates,
        vec![
            ("comment", "Some card payments fail."),
            ("status", "identified"),
            ("status", "investigating"),
        ]
    );
    let rendered = String::from_utf8_lossy(&body);
    for secret in [
        "10.0.0.4",
        "colleague@example.com",
        "person@example.com",
        "Secret db",
        "the failing payment",
        TRACE,
        &id(COLLEAGUE),
        &id(ME),
        &rule.as_uuid().to_string(),
        "Admin console",
    ] {
        assert!(!rendered.contains(secret), "{secret} leaked: {rendered}");
    }
}

#[tokio::test]
async fn a_private_or_unknown_page_is_not_found() {
    let harness = Harness::new();
    harness.a_page("hidden", false).await;

    let (private, _) = harness
        .public(Method::GET, "/api/status/hidden", None)
        .await;
    let (unknown, _) = harness
        .public(Method::GET, "/api/status/nobody-here", None)
        .await;
    let (misshapen, _) = harness.public(Method::GET, "/api/status/NOPE", None).await;

    assert_eq!(private, StatusCode::NOT_FOUND);
    assert_eq!(unknown, StatusCode::NOT_FOUND);
    assert_eq!(misshapen, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_member_previews_a_page_before_it_is_public() {
    let harness = Harness::new();
    let page = harness.a_page("draft-page", false).await;
    harness
        .a_component(
            &page,
            json!({"name": "Website", "link": {"kind": "manual"}, "manual_status": "degraded"}),
        )
        .await;

    let (previewed, shown) = harness
        .member(Method::GET, &format!("{}/preview", page_at(&page)), None)
        .await;
    let (nobody, _) = harness
        .public(Method::GET, &format!("{}/preview", page_at(&page)), None)
        .await;

    assert_eq!(previewed, StatusCode::OK, "{shown}");
    assert_eq!(shown["overall"], "degraded");
    assert_eq!(
        shown["components"],
        json!([{"name": "Website", "status": "degraded"}])
    );
    assert_eq!(nobody, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn subscribing_always_answers_accepted() {
    let harness = Harness::new();
    harness.a_page("acme", true).await;
    harness.a_page("hidden", false).await;

    let mut answers = Vec::new();
    for slug in ["acme", "acme", "hidden", "nobody-here"] {
        let (status, _) = harness
            .public(
                Method::POST,
                &format!("/api/status/{slug}/subscribe"),
                Some(json!({"email": "ada@example.com"})),
            )
            .await;
        answers.push(status);
    }
    let (misshapen, _) = harness
        .public(
            Method::POST,
            "/api/status/acme/subscribe",
            Some(json!({"email": "not an address"})),
        )
        .await;

    assert!(answers.iter().all(|status| *status == StatusCode::ACCEPTED));
    assert_eq!(misshapen, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(harness.status.subscribers().len(), 1);
    assert_eq!(
        harness.status.letters().len(),
        1,
        "one confirmation, deduped"
    );
}

#[tokio::test]
async fn confirmation_and_unsubscribe_links_land_on_the_page_and_refuse_forgeries() {
    let harness = Harness::new();
    harness.a_page("acme", true).await;
    let (page, subscriber) = harness.subscribed("acme", "ada@example.com").await;
    let confirm = signed(Purpose::Confirm, page, subscriber, NOW + 60);
    let expired = signed(Purpose::Confirm, page, subscriber, NOW);
    let leave = signed(Purpose::Unsubscribe, page, subscriber, NOW + 60);
    let flipped = if confirm.ends_with('0') { '1' } else { '0' };
    let forged = format!("{}{flipped}", &confirm[..confirm.len() - 1]);

    let mut seen = Vec::new();
    for uri in [
        format!("/api/status/confirm?token={forged}"),
        format!("/api/status/confirm?token={expired}"),
        format!("/api/status/unsubscribe?token={confirm}"),
        format!("/api/status/confirm?token={confirm}"),
    ] {
        let response = harness
            .raw(OrgRole::Member, bare(Method::GET, &uri, None))
            .await;
        seen.push((
            response.status(),
            response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned),
        ));
    }
    let confirmed = harness.status.subscribers()[0].confirmed;
    let left = harness
        .raw(
            OrgRole::Member,
            bare(
                Method::GET,
                &format!("/api/status/unsubscribe?token={leave}"),
                None,
            ),
        )
        .await;

    assert_eq!(
        seen,
        vec![
            (StatusCode::FORBIDDEN, None),
            (StatusCode::GONE, None),
            (StatusCode::FORBIDDEN, None),
            (
                StatusCode::SEE_OTHER,
                Some("/status/acme?notice=confirmed".to_owned())
            ),
        ]
    );
    assert!(confirmed);
    assert_eq!(left.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        left.headers()
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some("/status/acme?notice=unsubscribed")
    );
    assert!(harness.status.subscribers().is_empty());
}

#[tokio::test]
async fn a_public_comment_queues_one_email_per_confirmed_subscriber_and_a_private_one_none() {
    let harness = Harness::new();
    harness.a_page("acme", true).await;
    harness.confirmed("acme", "one@example.com").await;
    harness.confirmed("acme", "two@example.com").await;
    harness.subscribed("acme", "pending@example.com").await;
    let incident = harness
        .declared(json!({"title": "Payments fail", "public": true}))
        .await;
    let path = incident_at(&incident);
    let before = harness.status.letters().len();

    harness
        .member(
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "internal note"})),
        )
        .await;
    let after_private = harness.status.letters().len();
    harness
        .member(
            Method::POST,
            &format!("{path}/comments"),
            Some(json!({"body": "Some card payments fail.", "public": true})),
        )
        .await;
    let worked = harness.state_app().announce_status().await.expect("worked");

    assert_eq!(after_private, before, "a private comment mails nobody");
    assert_eq!(harness.status.letters().len(), before + 2);
    assert!(matches!(
        worked,
        ferristrace_domain::Heralded::Worked { .. }
    ));
    let lettered: Vec<String> = harness
        .courier
        .lettered()
        .into_iter()
        .filter(|(_, letter)| {
            matches!(&letter.body, ferristrace_domain::LetterBody::Update(bulletin)
                if &*bulletin.text == "Some card payments fail.")
        })
        .map(|(to, _)| to.to_string())
        .collect();
    assert_eq!(lettered.len(), 2);
    assert!(lettered.contains(&"one@example.com".to_owned()));
    assert!(lettered.contains(&"two@example.com".to_owned()));
}

#[tokio::test]
async fn a_published_postmortem_of_a_public_incident_is_linked_from_the_page() {
    let harness = Harness::new();
    harness.a_page("acme", true).await;
    let incident = harness
        .declared(json!({"title": "Payments fail", "public": true}))
        .await;
    let incident_id = IncidentId::new(
        incident["id"]
            .as_str()
            .and_then(|text| Uuid::try_parse(text).ok())
            .expect("an id"),
    );
    Postmortems::create(
        &harness.postmortems,
        &Postmortem {
            id: PostmortemId::fresh(),
            org: org(MINE),
            incident: incident_id,
            status: PostmortemStatus::Published,
            body: PostmortemBody::try_from("# Payments fail\nOwner: person@example.com")
                .expect("a body"),
            author: Some(account(ME)),
            edited: true,
            created_ms: 1,
            updated_ms: 2,
            published_ms: Some(3),
        },
    )
    .await
    .expect("kept");

    let (_, shown) = harness.public(Method::GET, "/api/status/acme", None).await;
    let url = shown["incidents"][0]["postmortem_url"]
        .as_str()
        .expect("a link")
        .to_owned();
    let (served, postmortem) = harness
        .public(
            Method::GET,
            &format!("/api/status/acme/postmortems/{incident_id}"),
            None,
        )
        .await;

    assert_eq!(url, format!("/status/acme/postmortems/{incident_id}"));
    assert_eq!(served, StatusCode::OK, "{postmortem}");
    assert_eq!(postmortem["title"], "Payments fail");
    assert_eq!(postmortem["body"], "# Payments fail\nOwner: [redacted]");
    assert_eq!(postmortem["published_ms"], 3);
}

#[tokio::test]
async fn spec_l2_8_every_status_page_write_is_audited() {
    let harness = Harness::new();
    let page = harness.a_page("acme", true).await;
    let component = harness
        .a_component(
            &page,
            json!({"name": "Website", "link": {"kind": "manual"}}),
        )
        .await;
    let component_path = format!(
        "{}/{}",
        components(&page),
        component["id"].as_str().expect("an id")
    );
    harness
        .admin(Method::PATCH, &component_path, Some(json!({"name": "Web"})))
        .await;
    harness.admin(Method::DELETE, &component_path, None).await;
    harness
        .admin(
            Method::PATCH,
            &page_at(&page),
            Some(json!({"public": false})),
        )
        .await;
    harness.admin(Method::DELETE, &page_at(&page), None).await;

    let logged: Vec<(Action, TargetKind)> = harness
        .notebook
        .entries()
        .into_iter()
        .map(|entry| (entry.action, entry.target.kind))
        .collect();
    assert_eq!(
        logged,
        vec![
            (Action::StatusPageCreated, TargetKind::StatusPage),
            (Action::StatusComponentCreated, TargetKind::StatusComponent),
            (Action::StatusComponentUpdated, TargetKind::StatusComponent),
            (Action::StatusComponentDeleted, TargetKind::StatusComponent),
            (Action::StatusPageUpdated, TargetKind::StatusPage),
            (Action::StatusPageDeleted, TargetKind::StatusPage),
        ]
    );
}
