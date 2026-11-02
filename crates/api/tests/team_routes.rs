use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, team_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Agent, LiveSession, Membership, Memberships, Named, NoPhotos, OrgRole, Owned,
    ProjectId, ProvisionPolicy, Session, SessionError, SessionId, SessionRoll, SessionStore,
    SessionToken, Squads, StoreError, Team, TeamId, TeamName, TenantId, UnixSeconds, WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const BACKEND: u128 = 11;
const FOUNDED: u64 = 1_700_000_000_123_456_789;

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
    renamed: Vec<(TeamId, String)>,
    disbanded: Vec<TeamId>,
    seated: Vec<(TeamId, Vec<AccountId>)>,
    owned: Vec<(TeamId, Vec<ProjectId>)>,
}

#[derive(Debug)]
struct Kept {
    teams: Mutex<Vec<Team>>,
    written: Mutex<Written>,
    taken: bool,
}

impl Kept {
    fn of(teams: Vec<Team>) -> Arc<Self> {
        Arc::new(Self {
            teams: Mutex::new(teams),
            written: Mutex::default(),
            taken: false,
        })
    }

    fn where_the_name_is_taken() -> Arc<Self> {
        Arc::new(Self {
            teams: Mutex::new(vec![backend()]),
            written: Mutex::default(),
            taken: true,
        })
    }

    fn holds(&self, team: TeamId) -> bool {
        self.teams
            .lock()
            .expect("the lock")
            .iter()
            .any(|one| one.id == team)
    }
}

impl Squads for Kept {
    async fn of(&self, _org: TenantId) -> Result<Vec<Team>, StoreError> {
        Ok(self.teams.lock().expect("the lock").clone())
    }

    async fn create(&self, _org: TenantId, name: &TeamName) -> Result<Named, StoreError> {
        if self.taken {
            return Ok(Named::Taken);
        }
        self.written
            .lock()
            .expect("the lock")
            .created
            .push(name.as_str().to_owned());
        self.teams.lock().expect("the lock").push(Team {
            id: TeamId::new(Uuid::from_u128(99)),
            name: name.as_str().into(),
            created_at_unix_nanos: FOUNDED,
            members: Vec::new(),
            projects: Vec::new(),
        });
        Ok(Named::Free)
    }

    async fn rename(
        &self,
        _org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> Result<Named, StoreError> {
        if self.taken {
            return Ok(Named::Taken);
        }
        self.written
            .lock()
            .expect("the lock")
            .renamed
            .push((team, name.as_str().to_owned()));
        Ok(Named::Free)
    }

    async fn disband(&self, _org: TenantId, team: TeamId) -> Result<bool, StoreError> {
        self.written.lock().expect("the lock").disbanded.push(team);
        let there = self.holds(team);
        self.teams
            .lock()
            .expect("the lock")
            .retain(|one| one.id != team);
        Ok(there)
    }

    async fn seat(
        &self,
        _org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> Result<bool, StoreError> {
        self.written
            .lock()
            .expect("the lock")
            .seated
            .push((team, members.to_vec()));
        Ok(self.holds(team))
    }

    async fn own(
        &self,
        _org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> Result<bool, StoreError> {
        self.written
            .lock()
            .expect("the lock")
            .owned
            .push((team, projects.to_vec()));
        Ok(self.holds(team))
    }
}

fn backend() -> Team {
    Team {
        id: TeamId::new(Uuid::from_u128(BACKEND)),
        name: "Backend".into(),
        created_at_unix_nanos: FOUNDED,
        members: vec![AccountId::new(Uuid::from_u128(9))],
        projects: vec![Owned {
            id: ProjectId::new(Uuid::from_u128(7)),
            name: "storefront".into(),
        }],
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

impl RepoFor<domain::Teams> for Fakes {
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
        oidc: None,
        provision: ProvisionPolicy::default(),
    };
    AppState::new(
        Arc::new(Ferristrace::new(fakes, adapters)),
        SessionCookie::default(),
    )
}

async fn served(role: OrgRole, kept: &Arc<Kept>, request: Request<Body>) -> (StatusCode, String) {
    let response = team_router(state(role, kept))
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

fn teams_of(org: u128) -> String {
    format!("/api/orgs/{}/teams", id(org))
}

fn team_of(org: u128, team: u128) -> String {
    format!("/api/orgs/{}/teams/{}", id(org), id(team))
}

#[tokio::test]
async fn a_member_reads_the_teams_with_who_is_on_them_and_what_they_own() {
    let kept = Kept::of(vec![backend()]);

    let (status, body) = served(
        OrgRole::Member,
        &kept,
        asking(Method::GET, &teams_of(MINE), None, true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let team = &read(&body)["teams"][0];
    assert_eq!(team["id"], id(BACKEND));
    assert_eq!(team["name"], "Backend");
    assert_eq!(team["members"][0], id(9));
    assert_eq!(team["projects"][0]["name"], "storefront");
    assert_eq!(
        team["created_at_ms"], 1_700_000_000_123u64,
        "a time a browser can hold without losing digits"
    );
}

#[tokio::test]
async fn a_request_from_another_origin_never_reaches_the_teams() {
    let kept = Kept::of(vec![backend()]);

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(Method::GET, &teams_of(MINE), None, false),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_teams_of_an_organisation_this_session_does_not_belong_to_are_not_found() {
    let kept = Kept::of(vec![backend()]);

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(Method::GET, &teams_of(THEIRS), None, true),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
}

#[tokio::test]
async fn an_administrator_creates_a_team_and_is_answered_with_them_all() {
    let kept = Kept::of(vec![backend()]);

    let (status, body) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::POST,
            &teams_of(MINE),
            Some(r#"{"name":"Payments"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let teams = read(&body);
    assert_eq!(
        teams["teams"].as_array().expect("an array").len(),
        2,
        "the answer carries the whole screen, not the row that moved"
    );
    assert_eq!(
        kept.written.lock().expect("the lock").created,
        vec!["Payments".to_owned()]
    );
}

#[tokio::test]
async fn a_member_arranges_nothing() {
    let kept = Kept::of(vec![backend()]);

    let (status, body) = served(
        OrgRole::Member,
        &kept,
        asking(
            Method::POST,
            &teams_of(MINE),
            Some(r#"{"name":"Payments"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("administrator"), "{body}");
    assert!(kept.written.lock().expect("the lock").created.is_empty());
}

#[tokio::test]
async fn a_blank_name_is_refused_with_the_rule_that_refused_it() {
    let kept = Kept::of(vec![backend()]);

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::POST,
            &teams_of(MINE),
            Some(r#"{"name":"  "}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("needs a name"), "{body}");
    assert!(kept.written.lock().expect("the lock").created.is_empty());
}

#[tokio::test]
async fn a_name_the_organisation_already_holds_is_a_conflict() {
    let kept = Kept::where_the_name_is_taken();

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::POST,
            &teams_of(MINE),
            Some(r#"{"name":"Backend"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already holds a team"), "{body}");
}

#[tokio::test]
async fn an_administrator_renames_a_team() {
    let kept = Kept::of(vec![backend()]);

    let (status, _) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::PATCH,
            &team_of(MINE, BACKEND),
            Some(r#"{"name":"Platform"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kept.written.lock().expect("the lock").renamed,
        vec![(TeamId::new(Uuid::from_u128(BACKEND)), "Platform".to_owned())]
    );
}

#[tokio::test]
async fn an_administrator_disbands_a_team_and_reads_what_is_left() {
    let kept = Kept::of(vec![backend()]);

    let (status, body) = served(
        OrgRole::Admin,
        &kept,
        asking(Method::DELETE, &team_of(MINE, BACKEND), None, true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        read(&body)["teams"]
            .as_array()
            .expect("an array")
            .is_empty()
    );
}

#[tokio::test]
async fn a_team_this_organisation_does_not_hold_is_not_found() {
    let kept = Kept::of(vec![backend()]);

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(Method::DELETE, &team_of(MINE, 404), None, true),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn who_is_on_a_team_is_put_whole() {
    let kept = Kept::of(vec![backend()]);

    let (status, _) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::PUT,
            &format!("{}/members", team_of(MINE, BACKEND)),
            Some(&format!(r#"{{"members":["{}","{}"]}}"#, id(9), id(10))),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kept.written.lock().expect("the lock").seated,
        vec![(
            TeamId::new(Uuid::from_u128(BACKEND)),
            vec![
                AccountId::new(Uuid::from_u128(9)),
                AccountId::new(Uuid::from_u128(10)),
            ]
        )]
    );
}

#[tokio::test]
async fn seating_nobody_empties_the_team_rather_than_being_refused() {
    let kept = Kept::of(vec![backend()]);

    let (status, _) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::PUT,
            &format!("{}/members", team_of(MINE, BACKEND)),
            Some(r#"{"members":[]}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kept.written.lock().expect("the lock").seated,
        vec![(TeamId::new(Uuid::from_u128(BACKEND)), Vec::new())]
    );
}

#[tokio::test]
async fn what_a_team_owns_is_put_whole() {
    let kept = Kept::of(vec![backend()]);

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::PUT,
            &format!("{}/projects", team_of(MINE, BACKEND)),
            Some(&format!(r#"{{"projects":["{}"]}}"#, id(7))),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        kept.written.lock().expect("the lock").owned,
        vec![(
            TeamId::new(Uuid::from_u128(BACKEND)),
            vec![ProjectId::new(Uuid::from_u128(7))]
        )]
    );
}

#[tokio::test]
async fn a_member_seats_nobody() {
    let kept = Kept::of(vec![backend()]);

    let (status, _) = served(
        OrgRole::Member,
        &kept,
        asking(
            Method::PUT,
            &format!("{}/members", team_of(MINE, BACKEND)),
            Some(r#"{"members":[]}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(kept.written.lock().expect("the lock").seated.is_empty());
}
