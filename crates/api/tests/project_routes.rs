use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, catalogue_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Agent, Assigned, Created, LiveSession, Membership, Memberships, NoPhotos, OrgRole,
    OwnerTeam, Project, ProjectId, ProjectName, Projects, ProvisionPolicy, Renamed, Session,
    SessionError, SessionId, SessionRoll, SessionStore, SessionToken, StoreError, TeamId, TenantId,
    UnixSeconds, WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const STOREFRONT: u128 = 7;
const BACKEND: u128 = 11;
const CREATED: u64 = 1_700_000_000_123_456_789;

fn id(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
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

#[derive(Debug, Default)]
struct Written {
    created: Vec<String>,
    renamed: Vec<(ProjectId, String)>,
    forgotten: Vec<ProjectId>,
    assigned: Vec<(ProjectId, Option<TeamId>)>,
}

#[derive(Debug)]
struct Kept {
    written: Mutex<Written>,
    created: Created,
    renamed: Renamed,
    forgotten: bool,
    assigned: Assigned,
    team: Option<OwnerTeam>,
}

impl Kept {
    fn holding() -> Arc<Self> {
        Arc::new(Self::bare())
    }

    fn owned() -> Arc<Self> {
        Arc::new(Self {
            team: Some(OwnerTeam {
                id: TeamId::new(Uuid::from_u128(BACKEND)),
                name: "Backend".into(),
            }),
            ..Self::bare()
        })
    }

    fn where_the_name_is_taken() -> Arc<Self> {
        Arc::new(Self {
            created: Created::Taken,
            renamed: Renamed::Taken,
            ..Self::bare()
        })
    }

    fn where_there_is_no_such_project() -> Arc<Self> {
        Arc::new(Self {
            renamed: Renamed::Gone,
            forgotten: false,
            assigned: Assigned::Gone,
            ..Self::bare()
        })
    }

    fn where_there_is_no_such_team() -> Arc<Self> {
        Arc::new(Self {
            assigned: Assigned::NoSuchTeam,
            ..Self::bare()
        })
    }

    fn bare() -> Self {
        Self {
            written: Mutex::default(),
            created: Created::Done,
            renamed: Renamed::Done,
            forgotten: true,
            assigned: Assigned::Done,
            team: None,
        }
    }

    fn written(&self) -> std::sync::MutexGuard<'_, Written> {
        self.written.lock().expect("the lock")
    }
}

impl Projects for Kept {
    async fn of(&self, tenant: TenantId) -> Result<Vec<Project>, StoreError> {
        Ok(vec![Project {
            id: ProjectId::new(Uuid::from_u128(STOREFRONT)),
            org: tenant,
            name: "storefront".into(),
            created_at_unix_nanos: CREATED,
            team: self.team.clone(),
            keys: 2,
        }])
    }

    async fn create(&self, _org: TenantId, name: &ProjectName) -> Result<Created, StoreError> {
        self.written().created.push(name.as_str().to_owned());
        Ok(self.created)
    }

    async fn rename(
        &self,
        _org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> Result<Renamed, StoreError> {
        self.written()
            .renamed
            .push((project, name.as_str().to_owned()));
        Ok(self.renamed)
    }

    async fn forget(&self, _org: TenantId, project: ProjectId) -> Result<bool, StoreError> {
        self.written().forgotten.push(project);
        Ok(self.forgotten)
    }

    async fn assign(
        &self,
        _org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> Result<Assigned, StoreError> {
        self.written().assigned.push((project, team));
        Ok(self.assigned)
    }
}

fn asking(method: Method, uri: &str, body: Option<&str>, scripted: bool) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(COOKIE, format!("ft_session={TOKEN}"));
    if scripted {
        request = request.header("x-requested-with", "fetch");
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    request
        .body(body.map_or_else(Body::empty, |json| Body::from(json.to_owned())))
        .expect("a request")
}

struct Fakes {
    role: OrgRole,
    kept: Arc<Kept>,
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

impl RepoFor<domain::Projects> for Fakes {
    type Repo = Arc<Kept>;

    fn build(&self, (): &()) -> Self::Repo {
        Arc::clone(&self.kept)
    }
}

struct TestStack;

impl Stack for TestStack {
    type Repos = Fakes;
    type Clock = Inert;
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
}

fn state(role: OrgRole, kept: &Arc<Kept>) -> AppState<TestStack> {
    let fakes = Fakes {
        role,
        kept: Arc::clone(kept),
    };
    let adapters = Adapters {
        clock: Inert::default(),
        hasher: Inert::default(),
        mailer: Inert::default(),
        photos: NoPhotos,
        meter: Inert::default(),
        span_store: None,
        issue_store: None,
        log_store: None,
        metric_store: None,
        oidc: None,
        provision: ProvisionPolicy::default(),
    };
    AppState::new(
        Arc::new(Ferristrace::new(fakes, adapters)),
        SessionCookie::default(),
    )
}

async fn served(role: OrgRole, kept: &Arc<Kept>, request: Request<Body>) -> (StatusCode, String) {
    let response = catalogue_router(state(role, kept))
        .oneshot(request)
        .await
        .expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("text"))
}

fn read(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("json")
}

fn projects_of(org: u128) -> String {
    format!("/api/orgs/{}/projects", id(org))
}

fn project_of(org: u128, project: u128) -> String {
    format!("/api/orgs/{}/projects/{}", id(org), id(project))
}

#[tokio::test]
async fn a_member_reads_the_projects_with_their_team_and_their_keys() {
    let kept = Kept::owned();

    let (status, body) = served(
        OrgRole::Member,
        &kept,
        asking(Method::GET, &projects_of(MINE), None, true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let project = &read(&body)["projects"][0];
    assert_eq!(project["id"], id(STOREFRONT));
    assert_eq!(project["name"], "storefront");
    assert_eq!(project["keys"], 2);
    assert_eq!(project["team"]["name"], "Backend");
    assert_eq!(
        project["created_at_ms"], 1_700_000_000_123u64,
        "a time a browser can hold without losing digits"
    );
}

#[tokio::test]
async fn a_project_no_team_owns_carries_a_null_team() {
    let kept = Kept::holding();

    let (status, body) = served(
        OrgRole::Member,
        &kept,
        asking(Method::GET, &projects_of(MINE), None, true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(read(&body)["projects"][0]["team"].is_null());
}

#[tokio::test]
async fn a_request_from_another_origin_never_reaches_the_projects() {
    let kept = Kept::holding();

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(Method::GET, &projects_of(MINE), None, false),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_projects_of_an_organisation_this_session_does_not_belong_to_are_not_found() {
    let kept = Kept::holding();

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(Method::GET, &projects_of(THEIRS), None, true),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
}

#[tokio::test]
async fn an_administrator_creates_a_project_and_is_answered_with_the_catalogue() {
    let kept = Kept::holding();

    let (status, body) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::POST,
            &projects_of(MINE),
            Some(r#"{"name":"payments-eu"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        read(&body)["projects"].as_array().expect("an array").len(),
        1
    );
    assert_eq!(kept.written().created, vec!["payments-eu".to_owned()]);
}

#[tokio::test]
async fn a_name_is_lowered_rather_than_refused_for_its_case() {
    let kept = Kept::holding();

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::POST,
            &projects_of(MINE),
            Some(r#"{"name":"  Payments-EU  "}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(kept.written().created, vec!["payments-eu".to_owned()]);
}

#[tokio::test]
async fn a_name_nothing_could_be_grepped_for_is_refused_with_the_rule() {
    let kept = Kept::holding();

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::POST,
            &projects_of(MINE),
            Some(r#"{"name":"pay ments"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body.contains("lowercase letters, digits and dashes"),
        "{body}"
    );
    assert!(kept.written().created.is_empty());
}

#[tokio::test]
async fn a_member_arranges_nothing() {
    let kept = Kept::holding();

    let (status, body) = served(
        OrgRole::Member,
        &kept,
        asking(
            Method::POST,
            &projects_of(MINE),
            Some(r#"{"name":"payments-eu"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("administrator"), "{body}");
    assert!(kept.written().created.is_empty());
}

#[tokio::test]
async fn a_name_the_organisation_already_holds_is_a_conflict() {
    let kept = Kept::where_the_name_is_taken();

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::POST,
            &projects_of(MINE),
            Some(r#"{"name":"storefront"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already holds a project"), "{body}");
}

#[tokio::test]
async fn an_administrator_renames_a_project() {
    let kept = Kept::holding();

    let (status, _) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::PATCH,
            &project_of(MINE, STOREFRONT),
            Some(r#"{"name":"shopfront"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kept.written().renamed,
        vec![(
            ProjectId::new(Uuid::from_u128(STOREFRONT)),
            "shopfront".to_owned()
        )]
    );
}

#[tokio::test]
async fn a_project_this_organisation_does_not_hold_is_not_found() {
    let kept = Kept::where_there_is_no_such_project();

    for request in [
        asking(
            Method::PATCH,
            &project_of(MINE, 404),
            Some(r#"{"name":"gone"}"#),
            true,
        ),
        asking(Method::DELETE, &project_of(MINE, 404), None, true),
        asking(
            Method::PUT,
            &format!("{}/team", project_of(MINE, 404)),
            Some(r#"{"team":null}"#),
            true,
        ),
    ] {
        let (status, _) = served(OrgRole::Owner, &kept, request).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn an_administrator_forgets_a_project_and_reads_what_is_left() {
    let kept = Kept::holding();

    let (status, body) = served(
        OrgRole::Admin,
        &kept,
        asking(Method::DELETE, &project_of(MINE, STOREFRONT), None, true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(read(&body)["projects"].is_array());
    assert_eq!(
        kept.written().forgotten,
        vec![ProjectId::new(Uuid::from_u128(STOREFRONT))]
    );
}

#[tokio::test]
async fn a_project_is_handed_to_a_team() {
    let kept = Kept::holding();

    let (status, _) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::PUT,
            &format!("{}/team", project_of(MINE, STOREFRONT)),
            Some(&format!(r#"{{"team":"{}"}}"#, id(BACKEND))),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kept.written().assigned,
        vec![(
            ProjectId::new(Uuid::from_u128(STOREFRONT)),
            Some(TeamId::new(Uuid::from_u128(BACKEND)))
        )]
    );
}

#[tokio::test]
async fn a_null_team_takes_the_project_back_from_whoever_held_it() {
    let kept = Kept::holding();

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::PUT,
            &format!("{}/team", project_of(MINE, STOREFRONT)),
            Some(r#"{"team":null}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kept.written().assigned,
        vec![(ProjectId::new(Uuid::from_u128(STOREFRONT)), None)]
    );
}

#[tokio::test]
async fn a_team_of_another_organisation_cannot_be_handed_a_project() {
    let kept = Kept::where_there_is_no_such_team();

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::PUT,
            &format!("{}/team", project_of(MINE, STOREFRONT)),
            Some(&format!(r#"{{"team":"{}"}}"#, id(404))),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("no such team"), "{body}");
}
