use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, me_router, workspace_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, ControlPlane, Email, Membership, Memberships, Minted, NoPhotos, OrgName, OrgRole,
    ProjectId, ProjectName, ProvisionError, Revoked, Session, SessionError, SessionStore,
    SessionSwitch, SessionToken, TenantId, UnixSeconds, WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

#[allow(dead_code)]
#[path = "support/identity.rs"]
mod identity;

use identity::{NoInbox, Nowhere};

const TOKEN: &str = "ft.person.0123456789";
const ACCOUNT: u128 = 9;
const ACME: u128 = 1;
const GLOBEX: u128 = 2;
const STRANGERS: u128 = 3;

#[derive(Debug)]
struct World {
    active: Option<TenantId>,
    memberships: Vec<Membership>,
    founded: Vec<(TenantId, String)>,
}

fn a_world() -> Arc<Mutex<World>> {
    Arc::new(Mutex::new(World {
        active: Some(an_org(ACME)),
        memberships: vec![
            Membership {
                org: an_org(ACME),
                name: "Acme".into(),
                role: OrgRole::Owner,
            },
            Membership {
                org: an_org(GLOBEX),
                name: "Globex".into(),
                role: OrgRole::Member,
            },
        ],
        founded: Vec::new(),
    }))
}

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn the_account() -> AccountId {
    AccountId::new(Uuid::from_u128(ACCOUNT))
}

#[derive(Clone)]
struct Sessions(Arc<Mutex<World>>);

impl SessionStore for Sessions {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() != TOKEN {
            return Err(SessionError::Unknown);
        }
        Ok(Session {
            account: the_account(),
            email: Email::try_from("person@example.com").expect("an address"),
            name: None,
            photo: None,
            org: self.0.lock().expect("the lock").active,
            expires_at: UnixSeconds::new(0),
        })
    }
}

impl SessionSwitch for Sessions {
    async fn switch(&self, _token: &SessionToken, org: TenantId) -> Result<(), SessionError> {
        self.0.lock().expect("the lock").active = Some(org);
        Ok(())
    }
}

#[derive(Clone)]
struct Belonging(Arc<Mutex<World>>);

impl Memberships for Belonging {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(self.0.lock().expect("the lock").memberships.clone())
    }
}

#[derive(Clone)]
struct Founder(Arc<Mutex<World>>);

impl ControlPlane for Founder {
    async fn founded(&self) -> Result<Option<TenantId>, ProvisionError> {
        Err(ProvisionError::Unreachable)
    }

    async fn found(&self, name: &OrgName, owner: &Email) -> Result<TenantId, ProvisionError> {
        assert_eq!(owner.as_str(), "person@example.com");
        let mut world = self.0.lock().expect("the lock");
        let org = an_org(100 + u128::try_from(world.founded.len()).expect("a small count"));
        world.founded.push((org, name.as_str().to_owned()));
        world.memberships.push(Membership {
            org,
            name: name.as_str().into(),
            role: OrgRole::Owner,
        });
        Ok(org)
    }

    async fn grant(&self, _: TenantId, _: &Email, _: OrgRole) -> Result<(), ProvisionError> {
        Err(ProvisionError::Unreachable)
    }

    async fn account_for(&self, _: &Email) -> Result<Option<AccountId>, ProvisionError> {
        Err(ProvisionError::Unreachable)
    }

    async fn create_account(&self, _: &Email) -> Result<AccountId, ProvisionError> {
        Err(ProvisionError::Unreachable)
    }

    async fn claim_project(
        &self,
        _org: TenantId,
        name: &ProjectName,
    ) -> Result<ProjectId, ProvisionError> {
        assert_eq!(name.as_str(), "default");
        Ok(ProjectId::new(Uuid::from_u128(500)))
    }

    async fn only_project(&self, _: TenantId) -> Result<Option<ProjectId>, ProvisionError> {
        Err(ProvisionError::Unreachable)
    }

    async fn issue_key(&self, _: TenantId, _: Option<ProjectId>) -> Result<Minted, ProvisionError> {
        Err(ProvisionError::Unreachable)
    }

    async fn revoke_key(&self, _: &str) -> Result<Option<Revoked>, ProvisionError> {
        Err(ProvisionError::Unreachable)
    }
}

struct Fakes(Arc<Mutex<World>>);

impl Untransacted for Fakes {}

impl RepoFor<domain::Sessions> for Fakes {
    type Repo = Sessions;

    fn build(&self, (): &()) -> Self::Repo {
        Sessions(Arc::clone(&self.0))
    }
}

impl RepoFor<domain::Memberships> for Fakes {
    type Repo = Belonging;

    fn build(&self, (): &()) -> Self::Repo {
        Belonging(Arc::clone(&self.0))
    }
}

impl RepoFor<domain::ControlPlane> for Fakes {
    type Repo = Founder;

    fn build(&self, (): &()) -> Self::Repo {
        Founder(Arc::clone(&self.0))
    }
}

impl RepoFor<domain::Audit> for Fakes {
    type Repo = Inert;

    fn build(&self, (): &()) -> Self::Repo {
        Inert::default()
    }
}

impl RepoFor<domain::Accounts> for Fakes {
    type Repo = Nowhere;

    fn build(&self, (): &()) -> Self::Repo {
        Nowhere
    }
}

impl RepoFor<domain::Inbox> for Fakes {
    type Repo = NoInbox;

    fn build(&self, (): &()) -> Self::Repo {
        NoInbox
    }
}

struct TestStack;

impl Stack for TestStack {
    type Repos = Fakes;
    type Clock = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = Inert;
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

fn app(world: &Arc<Mutex<World>>) -> Router {
    let adapters = Adapters {
        clock: Inert::default(),
        mailer: Inert::default(),
        webhooks: Inert::default(),
        photos: NoPhotos,
        meter: Inert::default(),
        span_store: None,
        issue_store: None,
        log_store: None,
        metric_store: None,
        users: Some(Bearers),
    };
    let state: AppState<TestStack> = AppState::new(Arc::new(Ferristrace::new(
        Fakes(Arc::clone(world)),
        adapters,
    )));
    me_router(state.clone()).merge(workspace_router(state))
}

fn request(method: Method, uri: &str, body: Option<String>, signed_in: bool) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-requested-with", "fetch");
    if signed_in {
        request = request.header(AUTHORIZATION, format!("Bearer {TOKEN}"));
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    request
        .body(body.map_or_else(Body::empty, Body::from))
        .expect("a request")
}

fn switching(org: u128, signed_in: bool) -> Request<Body> {
    request(
        Method::PUT,
        "/auth/session/org",
        Some(serde_json::json!({ "org": Uuid::from_u128(org) }).to_string()),
        signed_in,
    )
}

fn founding(name: &str, signed_in: bool) -> Request<Body> {
    request(
        Method::POST,
        "/api/orgs",
        Some(serde_json::json!({ "name": name }).to_string()),
        signed_in,
    )
}

async fn answer(app: &Router, request: Request<Body>) -> (StatusCode, String) {
    let response = app.clone().oneshot(request).await.expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("text"))
}

async fn standing(app: &Router) -> serde_json::Value {
    let (status, body) = answer(app, request(Method::GET, "/auth/me", None, true)).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_str(&body).expect("json")
}

#[tokio::test]
async fn switching_to_an_organisation_the_account_belongs_to_is_reflected_by_me() {
    let world = a_world();
    let app = app(&world);

    let (status, body) = answer(&app, switching(GLOBEX, true)).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
    assert_eq!(
        standing(&app).await["active_org"],
        serde_json::json!(Uuid::from_u128(GLOBEX).to_string())
    );
}

#[tokio::test]
async fn switching_to_an_organisation_the_account_is_not_in_is_not_found() {
    let world = a_world();
    let app = app(&world);

    let (status, _) = answer(&app, switching(STRANGERS, true)).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(world.lock().expect("the lock").active, Some(an_org(ACME)));
}

#[tokio::test]
async fn switching_without_a_session_is_unauthorised() {
    let world = a_world();

    let (status, _) = answer(&app(&world), switching(GLOBEX, false)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(world.lock().expect("the lock").active, Some(an_org(ACME)));
}

#[tokio::test]
async fn founding_an_organisation_lists_it_as_owned_and_active() {
    let world = a_world();
    let app = app(&world);

    let (status, body) = answer(&app, founding("Initech", true)).await;

    assert_eq!(status, StatusCode::CREATED);
    let created: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(created["id"], serde_json::json!(an_org(100).as_uuid()));
    assert_eq!(created["name"], "Initech");
    assert_eq!(
        created["project"],
        serde_json::json!(Uuid::from_u128(500).to_string())
    );
    let me = standing(&app).await;
    assert_eq!(me["active_org"], created["id"]);
    let listed = me["memberships"]
        .as_array()
        .expect("memberships")
        .iter()
        .find(|membership| membership["org"] == created["id"])
        .expect("the new organisation is listed");
    assert_eq!(listed["name"], "Initech");
    assert_eq!(listed["role"], "owner");
}

#[tokio::test]
async fn founding_with_a_bad_name_is_unprocessable_and_founds_nothing() {
    let world = a_world();
    let app = app(&world);

    let (blank, _) = answer(&app, founding("   ", true)).await;
    let (long, _) = answer(&app, founding(&"x".repeat(1000), true)).await;

    assert_eq!(blank, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(long, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(world.lock().expect("the lock").founded.is_empty());
}

#[tokio::test]
async fn founding_without_a_session_is_unauthorised() {
    let world = a_world();

    let (status, _) = answer(&app(&world), founding("Initech", false)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(world.lock().expect("the lock").founded.is_empty());
}
