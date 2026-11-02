use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, audit_router, key_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Notebook, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Actor, Agent, ApiKey, ApiKeys, ApiSecret, Assigned, Created, IssuedKey, KeyId,
    KeyName, KeyScopes, LiveSession, Membership, Memberships, NoPhotos, OrgRole, Project,
    ProjectId, ProjectName, Projects, ProvisionPolicy, ReadGrant, Renamed, Session, SessionError,
    SessionId, SessionRoll, SessionStore, SessionToken, StoreError, TeamId, TenantId, UnixSeconds,
    WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const ME: u128 = 9;
const EMAIL: &str = "person@example.com";

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
                account: AccountId::new(Uuid::from_u128(ME)),
                email: EMAIL.try_into().expect("an address"),
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

#[derive(Default)]
struct Vault {
    held: Mutex<Vec<(TenantId, ApiKey, String, bool)>>,
}

impl ApiKeys for Vault {
    async fn list(&self, org: TenantId) -> Result<Vec<ApiKey>, StoreError> {
        Ok(self
            .held
            .lock()
            .expect("the lock")
            .iter()
            .filter(|(held, _, _, revoked)| *held == org && !revoked)
            .map(|(_, key, _, _)| key.clone())
            .collect())
    }

    async fn issue(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        by: AccountId,
    ) -> Result<IssuedKey, StoreError> {
        let mut held = self.held.lock().expect("the lock");
        let seed = u128::try_from(held.len()).expect("a count") + 100;
        let prefix = format!("ft_live_{seed:012x}");
        let plaintext = format!("{prefix}.{seed:064x}");
        let key = ApiKey {
            id: KeyId::new(Uuid::from_u128(seed)),
            project,
            name: name.as_str().into(),
            prefix: prefix.into(),
            scopes: scopes.clone(),
            created_at_unix_nanos: 1,
            created_by: Some(by),
            last_used_at_unix_nanos: None,
        };
        held.push((org, key.clone(), plaintext.clone(), false));
        Ok(IssuedKey {
            key,
            secret: ApiSecret::from_plaintext(&plaintext),
        })
    }

    async fn revoke(&self, org: TenantId, id: KeyId) -> Result<bool, StoreError> {
        let mut held = self.held.lock().expect("the lock");
        Ok(held
            .iter_mut()
            .find(|(held, key, _, revoked)| *held == org && key.id == id && !revoked)
            .map(|(_, _, _, revoked)| *revoked = true)
            .is_some())
    }

    async fn grant_for(&self, secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(self
            .held
            .lock()
            .expect("the lock")
            .iter()
            .find(|(_, _, plaintext, revoked)| plaintext == secret.exposed() && !revoked)
            .map(|(org, key, _, _)| ReadGrant {
                org: *org,
                project: key.project,
            }))
    }
}

struct NoProjects;

impl Projects for NoProjects {
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
    vault: Arc<Vault>,
    notebook: Notebook,
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

impl RepoFor<domain::ApiKeys> for Fakes {
    type Repo = Arc<Vault>;

    fn build(&self, (): &()) -> Self::Repo {
        Arc::clone(&self.vault)
    }
}

impl RepoFor<domain::Projects> for Fakes {
    type Repo = NoProjects;

    fn build(&self, (): &()) -> Self::Repo {
        NoProjects
    }
}

impl RepoFor<domain::Audit> for Fakes {
    type Repo = Notebook;

    fn build(&self, (): &()) -> Self::Repo {
        self.notebook.clone()
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
    type Webhooks = Inert;
}

struct Harness {
    vault: Arc<Vault>,
    notebook: Notebook,
}

impl Harness {
    fn new() -> Self {
        let notebook = Notebook::default();
        notebook.label(Actor::Account(AccountId::new(Uuid::from_u128(ME))), EMAIL);
        Self {
            vault: Arc::default(),
            notebook,
        }
    }

    fn app(&self, role: OrgRole) -> Router {
        let adapters = Adapters {
            clock: Inert::default(),
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
                    vault: Arc::clone(&self.vault),
                    notebook: self.notebook.clone(),
                },
                adapters,
            )),
            SessionCookie::default(),
        );
        key_router(state.clone()).merge(audit_router(state))
    }

    async fn served(&self, role: OrgRole, request: Request<Body>) -> (StatusCode, String) {
        let response = self.app(role).oneshot(request).await.expect("a response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, String::from_utf8(body.to_vec()).expect("text"))
    }

    async fn issued(&self, name: &str) -> serde_json::Value {
        let (status, body) = self
            .served(
                OrgRole::Admin,
                as_session(
                    Method::POST,
                    &format!("/api/orgs/{}/keys", id(MINE)),
                    Some(&format!(
                        r#"{{"name":"{name}","scopes":["ingest","read"]}}"#
                    )),
                ),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        read(&body)
    }

    async fn audit(&self, role: OrgRole, query: &str) -> (StatusCode, String) {
        self.served(
            role,
            as_session(
                Method::GET,
                &format!("/api/orgs/{}/audit{query}", id(MINE)),
                None,
            ),
        )
        .await
    }

    async fn trail(&self, query: &str) -> serde_json::Value {
        let (status, body) = self.audit(OrgRole::Admin, query).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        read(&body)
    }
}

fn as_session(method: Method, uri: &str, body: Option<&str>) -> Request<Body> {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-requested-with", "fetch")
        .header(COOKIE, format!("ft_session={TOKEN}"));
    match body {
        Some(json) => request
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_owned())),
        None => request.body(Body::empty()),
    }
    .expect("a request")
}

fn read(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("json")
}

fn actions(trail: &serde_json::Value) -> Vec<String> {
    trail["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["action"].as_str().expect("an action").to_owned())
        .collect()
}

#[tokio::test]
async fn owners_and_admins_read_the_log_newest_first() {
    let harness = Harness::new();
    let issued = harness.issued("ci").await;
    let key = issued["key"]["id"].as_str().expect("an id").to_owned();
    let (revoked, _) = harness
        .served(
            OrgRole::Admin,
            as_session(
                Method::DELETE,
                &format!("/api/orgs/{}/keys/{key}", id(MINE)),
                None,
            ),
        )
        .await;
    assert_eq!(revoked, StatusCode::NO_CONTENT);

    for role in [OrgRole::Owner, OrgRole::Admin] {
        let (status, body) = harness.audit(role, "").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let trail = read(&body);

        assert_eq!(actions(&trail), vec!["key.revoked", "key.issued"]);
        let newest = &trail["entries"][0];
        assert_eq!(newest["actor"]["kind"], "account");
        assert_eq!(newest["actor"]["id"], id(ME));
        assert_eq!(newest["actor"]["label"], EMAIL);
        assert_eq!(newest["target"]["kind"], "key");
        assert_eq!(newest["target"]["id"], key);
        assert!(newest["at_ms"].is_u64());
        assert!(newest["id"].is_string());
        assert_eq!(trail["entries"][1]["detail"]["name"], "ci");
        assert_eq!(trail["next"], serde_json::Value::Null);
    }
}

#[tokio::test]
async fn a_member_is_refused_like_any_other_administrators_page() {
    let harness = Harness::new();
    harness.issued("ci").await;

    let (status, _) = harness.audit(OrgRole::Member, "").await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_stranger_is_told_there_is_no_such_organisation() {
    let harness = Harness::new();

    let (status, _) = harness
        .served(
            OrgRole::Owner,
            as_session(
                Method::GET,
                &format!("/api/orgs/{}/audit", id(THEIRS)),
                None,
            ),
        )
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_read_key_is_no_session_for_the_audit_log() {
    let harness = Harness::new();
    let secret = harness.issued("dashboards").await["secret"]
        .as_str()
        .expect("a secret")
        .to_owned();

    let (status, _) = harness
        .served(
            OrgRole::Owner,
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/orgs/{}/audit", id(MINE)))
                .header("x-requested-with", "fetch")
                .header(AUTHORIZATION, format!("Bearer {secret}"))
                .body(Body::empty())
                .expect("a request"),
        )
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_cursor_of_a_full_page_reads_the_rest() {
    let harness = Harness::new();
    for name in ["one", "two", "three"] {
        harness.issued(name).await;
    }

    let first = harness.trail("?limit=2").await;
    let next = first["next"].as_str().expect("a cursor").to_owned();
    let rest = harness.trail(&format!("?limit=2&before={next}")).await;

    let names = |trail: &serde_json::Value| -> Vec<String> {
        trail["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .map(|entry| entry["detail"]["name"].as_str().expect("a name").to_owned())
            .collect()
    };
    assert_eq!(names(&first), vec!["three", "two"]);
    assert_eq!(names(&rest), vec!["one"]);
    assert_eq!(rest["next"], serde_json::Value::Null);
}

#[tokio::test]
async fn the_log_filters_by_action_and_actor() {
    let harness = Harness::new();
    let issued = harness.issued("ci").await;
    let key = issued["key"]["id"].as_str().expect("an id").to_owned();
    harness
        .served(
            OrgRole::Admin,
            as_session(
                Method::DELETE,
                &format!("/api/orgs/{}/keys/{key}", id(MINE)),
                None,
            ),
        )
        .await;

    let issued_only = harness.trail("?action=key.issued").await;
    let mine = harness
        .trail(&format!("?actor_kind=account&actor_id={}", id(ME)))
        .await;
    let somebody_else = harness
        .trail(&format!("?actor_kind=account&actor_id={}", id(42)))
        .await;
    let the_operator = harness.trail("?actor_kind=operator").await;
    let blank = harness.trail("?action=&actor_kind=&before=").await;

    assert_eq!(actions(&issued_only), vec!["key.issued"]);
    assert_eq!(actions(&mine), vec!["key.revoked", "key.issued"]);
    assert!(actions(&somebody_else).is_empty());
    assert!(actions(&the_operator).is_empty());
    assert_eq!(actions(&blank).len(), 2);
}

#[tokio::test]
async fn a_query_the_log_cannot_answer_is_refused() {
    let harness = Harness::new();

    for query in [
        "?action=org.deleted",
        "?before=not-a-cursor",
        "?actor_kind=robot",
        "?actor_kind=account",
        &format!("?actor_id={}", id(ME)),
    ] {
        let (status, body) = harness.audit(OrgRole::Admin, query).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}: {body}");
    }
}

#[tokio::test]
async fn no_entry_carries_the_secret_of_the_key_it_records() {
    let harness = Harness::new();
    let issued = harness.issued("ci").await;
    let secret = issued["secret"].as_str().expect("a secret").to_owned();
    let (_, hidden) = secret.split_once('.').expect("a prefixed secret");

    let (status, body) = harness.audit(OrgRole::Owner, "").await;

    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(&secret), "{body}");
    assert!(!body.contains(hidden), "{body}");
    assert_eq!(harness.notebook.entries().len(), 1);
}
