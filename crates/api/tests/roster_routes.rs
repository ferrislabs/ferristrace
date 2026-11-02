use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, roster_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::organisation::{Holder, Joined};
use ferristrace_domain::{
    AccountId, Agent, Email, LiveSession, Member, Membership, Memberships, NoPhotos, OrgRole,
    ProvisionPolicy, RESET_PREFIX, Roll, SecretHash, Session, SessionError, SessionId, SessionRoll,
    SessionStore, SessionToken, StoreError, TenantId, UnixSeconds, WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ct_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const ME: u128 = 9;
const SOMEBODY: u128 = 10;
const JOINED: u64 = 1_700_000_000_123_456_789;

fn an_account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn an_address(local: &str) -> Email {
    Email::try_from(format!("{local}@example.com").as_str()).expect("an address")
}

fn a_member(seed: u128, role: OrgRole) -> Member {
    Member {
        account: an_account(seed),
        email: an_address(&format!("person{seed}")),
        name: None,
        photo: None,
        role,
        joined_at_unix_nanos: JOINED,
    }
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
                account: an_account(ME),
                email: an_address("me"),
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

#[derive(Debug)]
struct Kept {
    roll: Mutex<Vec<Member>>,
    holder: Option<Holder>,
    already_in: bool,
    digests: Mutex<Vec<SecretHash>>,
}

impl Kept {
    fn of(roll: Vec<Member>) -> Arc<Self> {
        Arc::new(Self {
            roll: Mutex::new(roll),
            holder: None,
            already_in: false,
            digests: Mutex::default(),
        })
    }

    fn awaiting() -> Arc<Self> {
        Self::of(Vec::new())
    }

    fn standing(account: AccountId) -> Arc<Self> {
        Arc::new(Self {
            roll: Mutex::new(Vec::new()),
            holder: Some(Holder {
                account,
                name: None,
                photo: None,
            }),
            already_in: false,
            digests: Mutex::default(),
        })
    }

    fn already_in(account: AccountId) -> Arc<Self> {
        Arc::new(Self {
            roll: Mutex::new(Vec::new()),
            holder: Some(Holder {
                account,
                name: None,
                photo: None,
            }),
            already_in: true,
            digests: Mutex::default(),
        })
    }

    fn roll(&self) -> Vec<Member> {
        self.roll.lock().expect("the lock").clone()
    }

    fn digests(&self) -> usize {
        self.digests.lock().expect("the lock").len()
    }
}

impl Roll for Kept {
    async fn of(&self, _org: TenantId) -> Result<Vec<Member>, StoreError> {
        Ok(self.roll())
    }

    async fn grant(
        &self,
        _org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> Result<Option<Member>, StoreError> {
        let mut held = self.roll.lock().expect("the lock");
        Ok(held
            .iter_mut()
            .find(|member| member.account == account)
            .map(|member| {
                member.role = role;
                member.clone()
            }))
    }

    async fn remove(&self, _org: TenantId, account: AccountId) -> Result<bool, StoreError> {
        let mut roll = self.roll.lock().expect("the lock");
        let before = roll.len();
        roll.retain(|member| member.account != account);
        Ok(roll.len() < before)
    }

    async fn standing_of(&self, _email: &Email) -> Result<Option<Holder>, StoreError> {
        Ok(self.holder.clone())
    }

    async fn create_locked(&self, _email: &Email) -> Result<AccountId, StoreError> {
        Ok(an_account(SOMEBODY))
    }

    async fn join(
        &self,
        _org: TenantId,
        _account: AccountId,
        role: OrgRole,
    ) -> Result<Option<Joined>, StoreError> {
        Ok((!self.already_in).then_some(Joined {
            role,
            joined_at_unix_nanos: JOINED,
        }))
    }

    async fn open_way_in(&self, _account: AccountId, digest: SecretHash) -> Result<(), StoreError> {
        self.digests.lock().expect("the lock").push(digest);
        Ok(())
    }
}

fn org(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

fn asking(method: Method, uri: &str, body: Option<&str>, scripted: bool) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(COOKIE, format!("ct_session={TOKEN}"));
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

impl RepoFor<domain::Roster> for Fakes {
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
    let response = roster_router(state(role, kept))
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

#[tokio::test]
async fn a_member_reads_the_whole_roll_of_its_own_organisation() {
    let kept = Kept::of(vec![
        a_member(ME, OrgRole::Owner),
        a_member(SOMEBODY, OrgRole::Member),
    ]);

    let (status, body) = served(
        OrgRole::Member,
        &kept,
        asking(
            Method::GET,
            &format!("/api/orgs/{}/members", org(MINE)),
            None,
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let roll = read(&body);
    assert_eq!(
        roll["members"][0]["account"],
        Uuid::from_u128(ME).to_string()
    );
    assert_eq!(roll["members"][0]["email"], "person9@example.com");
    assert_eq!(roll["members"][0]["role"], "owner");
    assert_eq!(
        roll["members"][0]["joined_at_ms"], 1_700_000_000_123u64,
        "a time a browser can hold without losing digits"
    );
    assert_eq!(roll["members"][1]["role"], "member");
}

#[tokio::test]
async fn a_request_from_another_origin_never_reaches_the_members() {
    let kept = Kept::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::GET,
            &format!("/api/orgs/{}/members", org(MINE)),
            None,
            false,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_members_of_an_organisation_this_session_does_not_belong_to_are_not_found() {
    let kept = Kept::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::GET,
            &format!("/api/orgs/{}/members", org(THEIRS)),
            None,
            true,
        ),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
}

#[tokio::test]
async fn an_administrator_changes_a_role_and_is_answered_with_the_member() {
    let kept = Kept::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let (status, body) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::PATCH,
            &format!("/api/orgs/{}/members/{}", org(MINE), org(SOMEBODY)),
            Some(r#"{"role":"admin"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(read(&body)["role"], "admin");
    assert_eq!(kept.roll()[0].role, OrgRole::Admin);
}

#[tokio::test]
async fn a_member_is_told_plainly_that_changing_a_role_is_not_its_to_do() {
    let kept = Kept::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let (status, body) = served(
        OrgRole::Member,
        &kept,
        asking(
            Method::PATCH,
            &format!("/api/orgs/{}/members/{}", org(MINE), org(SOMEBODY)),
            Some(r#"{"role":"admin"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("administrator"), "{body}");
    assert_eq!(kept.roll()[0].role, OrgRole::Member);
}

#[tokio::test]
async fn an_administrator_is_refused_the_owner_role_in_either_direction() {
    let kept = Kept::of(vec![a_member(SOMEBODY, OrgRole::Owner)]);

    let (status, body) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::PATCH,
            &format!("/api/orgs/{}/members/{}", org(MINE), org(SOMEBODY)),
            Some(r#"{"role":"member"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("owner"), "{body}");
    assert_eq!(kept.roll()[0].role, OrgRole::Owner);
}

#[tokio::test]
async fn the_last_owner_stays_an_owner_however_it_asks() {
    let kept = Kept::of(vec![a_member(ME, OrgRole::Owner)]);

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::PATCH,
            &format!("/api/orgs/{}/members/{}", org(MINE), org(ME)),
            Some(r#"{"role":"member"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("at least one owner"), "{body}");
    assert_eq!(kept.roll()[0].role, OrgRole::Owner);
}

#[tokio::test]
async fn an_owner_steps_down_from_its_own_row_once_another_owner_stays() {
    let kept = Kept::of(vec![
        a_member(ME, OrgRole::Owner),
        a_member(SOMEBODY, OrgRole::Owner),
    ]);

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::PATCH,
            &format!("/api/orgs/{}/members/{}", org(MINE), org(ME)),
            Some(r#"{"role":"admin"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(read(&body)["role"], "admin");
}

#[tokio::test]
async fn an_administrator_walks_out_of_an_organisation_it_does_not_own() {
    let kept = Kept::of(vec![
        a_member(ME, OrgRole::Admin),
        a_member(SOMEBODY, OrgRole::Owner),
    ]);

    let (status, _) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::DELETE,
            &format!("/api/orgs/{}/members/{}", org(MINE), org(ME)),
            None,
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(kept.roll().len(), 1);
    assert_eq!(kept.roll()[0].role, OrgRole::Owner);
}

#[tokio::test]
async fn an_administrator_shows_a_member_the_door_and_the_answer_carries_no_body() {
    let kept = Kept::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let (status, body) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::DELETE,
            &format!("/api/orgs/{}/members/{}", org(MINE), org(SOMEBODY)),
            None,
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
    assert!(kept.roll().is_empty());
}

#[tokio::test]
async fn an_account_this_organisation_never_held_is_no_member_to_remove() {
    let kept = Kept::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::DELETE,
            &format!("/api/orgs/{}/members/{}", org(MINE), org(404)),
            None,
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(kept.roll().len(), 1);
}

#[tokio::test]
async fn an_unknown_address_is_admitted_with_the_one_token_that_sets_its_password() {
    let kept = Kept::awaiting();

    let (status, body) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::POST,
            &format!("/api/orgs/{}/members", org(MINE)),
            Some(r#"{"email":"new@example.com","role":"member"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let welcomed = read(&body);
    assert_eq!(welcomed["member"]["role"], "member");
    let token = welcomed["invite_token"]
        .as_str()
        .expect("a new account cannot sign in without one");
    assert!(token.starts_with(RESET_PREFIX), "{token}");
    assert_eq!(
        kept.digests(),
        1,
        "the control plane is handed the digest, never the token"
    );
}

#[tokio::test]
async fn an_address_that_already_has_an_account_joins_without_a_token() {
    let kept = Kept::standing(an_account(SOMEBODY));

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::POST,
            &format!("/api/orgs/{}/members", org(MINE)),
            Some(r#"{"email":"known@example.com","role":"admin"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let welcomed = read(&body);
    assert_eq!(welcomed["member"]["role"], "admin");
    assert!(welcomed["invite_token"].is_null());
}

#[tokio::test]
async fn an_address_already_on_the_roll_is_a_conflict_rather_than_a_second_invitation() {
    let kept = Kept::already_in(an_account(SOMEBODY));

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::POST,
            &format!("/api/orgs/{}/members", org(MINE)),
            Some(r#"{"email":"already@example.com","role":"member"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("already belongs"), "{body}");
}

#[tokio::test]
async fn an_administrator_cannot_invite_an_owner_and_no_account_is_made_for_one() {
    let kept = Kept::awaiting();

    let (status, _) = served(
        OrgRole::Admin,
        &kept,
        asking(
            Method::POST,
            &format!("/api/orgs/{}/members", org(MINE)),
            Some(r#"{"email":"new@example.com","role":"owner"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(kept.digests(), 0);
}

#[tokio::test]
async fn something_that_is_not_an_address_is_refused_before_anybody_is_admitted() {
    let kept = Kept::awaiting();

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(
            Method::POST,
            &format!("/api/orgs/{}/members", org(MINE)),
            Some(r#"{"email":"nobody","role":"member"}"#),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("address"), "{body}");
    assert_eq!(kept.digests(), 0);
}
