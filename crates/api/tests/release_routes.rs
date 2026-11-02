use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::{Request, StatusCode};
use ferristrace_api::{AppState, SessionCookie, release_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Agent, ApiKey, ApiKeys, ApiSecret, Counted, Deploy, DeployFeed, Deployed,
    Fingerprint, IssueFilter, IssueReader, IssuedKey, KeyId, KeyName, KeyScopes, LiveSession,
    Membership, Memberships, NoPhotos, Occurrence, OrgRole, ProjectId, ProvisionPolicy, ReadGrant,
    Release, ReleaseDetail, ReleaseFilter, ReleaseReader, Session, SessionError, SessionId,
    SessionRoll, SessionStore, SessionToken, StoreError, TenantId, Traffic, UnixSeconds,
    WhoamiError, Window,
};
use ferristrace_infra::FixedClock;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft_ss_0123456789";
const ORG_KEY: &str = "ft_rd_whole_org";
const PROJECT_KEY: &str = "ft_rd_one_project";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const OURS: u128 = 30;
const OTHER: u128 = 40;
const NOW: u64 = 1_700_000_600;

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
                email: ferristrace_domain::Email::try_from("person@example.com")
                    .expect("an address"),
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

struct MemberOfMine;

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: org_id(MINE),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

struct TwoReadKeys;

impl ApiKeys for TwoReadKeys {
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
            ORG_KEY => Some(ReadGrant::whole(org_id(MINE))),
            PROJECT_KEY => Some(ReadGrant {
                org: org_id(MINE),
                project: Some(project_id(OURS)),
            }),
            _ => None,
        })
    }
}

struct Fakes;

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
        MemberOfMine
    }
}

impl RepoFor<domain::ApiKeys> for Fakes {
    type Repo = TwoReadKeys;

    fn build(&self, (): &()) -> Self::Repo {
        TwoReadKeys
    }
}

type Asked = (TenantId, Option<ProjectId>, Option<String>);

#[derive(Debug, Default)]
struct Shelf {
    asked: Mutex<Vec<Asked>>,
    windows: Mutex<Vec<Window>>,
    unknown: bool,
    broken: bool,
}

impl Shelf {
    fn asked(&self) -> Vec<Asked> {
        self.asked.lock().expect("the lock").clone()
    }

    fn windows(&self) -> Vec<Window> {
        self.windows.lock().expect("the lock").clone()
    }
}

struct Store(Arc<Shelf>);

fn a_release() -> Release {
    Release {
        service: "checkout".into(),
        version: "2.14.1".into(),
        environments: vec!["prod".into(), "staging".into()],
        first_seen_unix_nanos: 1_700_000_100_123_456_789,
        last_seen_unix_nanos: 1_700_000_500_000_000_000,
        requests: 900,
        errors: 36,
        new_issues: 1,
    }
}

fn an_issue() -> Counted {
    Counted {
        project: project_id(OURS),
        print: Fingerprint::try_from("00112233445566778899aabbccddeeff").expect("a print"),
        exception_type: "TimeoutError".into(),
        message: "deadline exceeded".into(),
        service: "checkout".into(),
        count: 14,
        first_seen_unix_nanos: 1_700_000_200_000_000_000,
        last_seen_unix_nanos: 1_700_000_300_000_000_000,
    }
}

impl IssueReader for Store {
    async fn issues(
        &self,
        _tenant: TenantId,
        _filter: &IssueFilter,
    ) -> Result<Vec<Counted>, StoreError> {
        Ok(Vec::new())
    }

    async fn occurrences(
        &self,
        _tenant: TenantId,
        _project: ProjectId,
        _print: Fingerprint,
        _window: Window,
        _limit: u32,
    ) -> Result<Vec<Occurrence>, StoreError> {
        Ok(Vec::new())
    }
}

impl DeployFeed for Store {
    async fn deployed(&self, _window: Window) -> Result<Vec<Deployed>, StoreError> {
        Ok(Vec::new())
    }
}

impl ReleaseReader for Store {
    async fn releases(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Release>, StoreError> {
        self.0.asked.lock().expect("the lock").push((
            org,
            filter.project,
            filter.service.as_deref().map(str::to_owned),
        ));
        self.0.windows.lock().expect("the lock").push(filter.window);
        if self.0.broken {
            return Err(StoreError::Unreachable);
        }
        Ok(vec![a_release()])
    }

    async fn release(
        &self,
        org: TenantId,
        service: &str,
        version: &str,
        project: Option<ProjectId>,
    ) -> Result<Option<ReleaseDetail>, StoreError> {
        self.0.asked.lock().expect("the lock").push((
            org,
            project,
            Some(format!("{service}@{version}")),
        ));
        Ok((!self.0.unknown).then(|| ReleaseDetail {
            release: a_release(),
            before: Traffic {
                requests: 1000,
                errors: 10,
            },
            after: Traffic {
                requests: 0,
                errors: 0,
            },
            introduced: vec![an_issue()],
        }))
    }

    async fn deploys(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Deploy>, StoreError> {
        self.0.asked.lock().expect("the lock").push((
            org,
            filter.project,
            filter.service.as_deref().map(str::to_owned),
        ));
        self.0.windows.lock().expect("the lock").push(filter.window);
        Ok(vec![Deploy {
            service: "checkout".into(),
            version: "2.14.1".into(),
            environment: "prod".into(),
            at_unix_nanos: 1_700_000_100_123_456_789,
        }])
    }
}

struct TestStack;

impl Stack for TestStack {
    type Repos = Fakes;
    type Clock = FixedClock;
    type Hasher = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = Store;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Oidc = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

fn state(shelf: Option<Arc<Shelf>>) -> AppState<TestStack> {
    let adapters = Adapters {
        clock: FixedClock::at(NOW),
        hasher: Inert::default(),
        mailer: Inert::default(),
        webhooks: Inert::default(),
        photos: NoPhotos,
        meter: Inert::default(),
        span_store: None,
        issue_store: shelf.map(Store),
        log_store: None,
        metric_store: None,
        oidc: None,
        provision: ProvisionPolicy::default(),
    };
    AppState::new(
        Arc::new(Ferristrace::new(Fakes, adapters)),
        SessionCookie::default(),
    )
}

enum Who {
    Session,
    Key(&'static str),
    Nobody,
}

fn looking(uri: &str, who: &Who) -> Request<Body> {
    let request = Request::builder().uri(uri);
    let request = match who {
        Who::Session => request
            .header("x-requested-with", "fetch")
            .header(COOKIE, format!("ft_session={TOKEN}")),
        Who::Key(key) => request.header(AUTHORIZATION, format!("Bearer {key}")),
        Who::Nobody => request.header("x-requested-with", "fetch"),
    };
    request.body(Body::empty()).expect("a request")
}

async fn served(shelf: Option<Arc<Shelf>>, request: Request<Body>) -> (StatusCode, String) {
    let response = release_router(state(shelf))
        .oneshot(request)
        .await
        .expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("text"))
}

fn org_id(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn project_id(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn releases_of(seed: u128, rest: &str) -> String {
    format!(
        "/api/orgs/{}/releases?from=1700000000000&to=1700001000000{rest}",
        Uuid::from_u128(seed)
    )
}

fn deploys_of(seed: u128, rest: &str) -> String {
    format!(
        "/api/orgs/{}/deploys?from=1700000000000&to=1700001000000{rest}",
        Uuid::from_u128(seed)
    )
}

fn one_of(seed: u128, service: &str, version: &str, rest: &str) -> String {
    format!(
        "/api/orgs/{}/releases/{service}/{version}?x=1{rest}",
        Uuid::from_u128(seed)
    )
}

#[tokio::test]
async fn a_member_lists_the_releases_in_the_agreed_shape() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(&releases_of(MINE, "&service=%20checkout%20"), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        found,
        serde_json::json!({
            "releases": [{
                "service": "checkout",
                "version": "2.14.1",
                "environments": ["prod", "staging"],
                "first_seen_at_ms": 1_700_000_100_123u64,
                "last_seen_at_ms": 1_700_000_500_000u64,
                "requests": 900,
                "errors": 36,
                "new_issues": 1,
            }]
        })
    );
    assert_eq!(
        shelf.asked(),
        vec![(org_id(MINE), None, Some("checkout".to_owned()))]
    );
    assert_eq!(
        shelf.windows()[0].from_unix_nanos(),
        1_700_000_000_000 * 1_000_000
    );
    assert_eq!(
        shelf.windows()[0].to_unix_nanos(),
        1_700_001_000_000 * 1_000_000
    );
}

#[tokio::test]
async fn a_member_reads_the_detail_of_one_release_with_the_issues_it_introduced() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(&one_of(MINE, "checkout", "2.14.1", ""), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        found,
        serde_json::json!({
            "service": "checkout",
            "version": "2.14.1",
            "environments": ["prod", "staging"],
            "first_seen_at_ms": 1_700_000_100_123u64,
            "last_seen_at_ms": 1_700_000_500_000u64,
            "requests": 900,
            "errors": 36,
            "new_issues": 1,
            "before": {"requests": 1000, "errors": 10, "error_rate": 0.01},
            "after": {"requests": 0, "errors": 0, "error_rate": null},
            "issues": [{
                "fingerprint": "00112233445566778899aabbccddeeff",
                "project": Uuid::from_u128(OURS).to_string(),
                "exception_type": "TimeoutError",
                "message": "deadline exceeded",
                "service": "checkout",
                "events": 14,
                "first_seen_at_ms": 1_700_000_200_000u64,
                "last_seen_at_ms": 1_700_000_300_000u64,
            }],
        })
    );
    assert_eq!(
        shelf.asked(),
        vec![(org_id(MINE), None, Some("checkout@2.14.1".to_owned()))]
    );
}

#[tokio::test]
async fn an_unknown_release_is_a_404() {
    let shelf = Arc::new(Shelf {
        unknown: true,
        ..Shelf::default()
    });

    let (status, body) = served(
        Some(shelf),
        looking(&one_of(MINE, "checkout", "9.9.9", ""), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, "no such release");
}

#[tokio::test]
async fn deploy_markers_are_served_with_their_instant_in_milliseconds() {
    let shelf = Arc::new(Shelf::default());

    let (status, body) = served(
        Some(Arc::clone(&shelf)),
        looking(&deploys_of(MINE, "&service=checkout"), &Who::Session),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        found,
        serde_json::json!({
            "deploys": [{
                "service": "checkout",
                "version": "2.14.1",
                "environment": "prod",
                "at_ms": 1_700_000_100_123u64,
            }]
        })
    );
    assert_eq!(
        shelf.asked(),
        vec![(org_id(MINE), None, Some("checkout".to_owned()))]
    );
}

#[tokio::test]
async fn a_backwards_or_missing_window_is_a_bad_request_before_the_store_is_asked() {
    let shelf = Arc::new(Shelf::default());
    let id = Uuid::from_u128(MINE);

    let (backwards, body) = served(
        Some(Arc::clone(&shelf)),
        looking(
            &format!("/api/orgs/{id}/releases?from=20&to=10"),
            &Who::Session,
        ),
    )
    .await;
    let (missing, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&format!("/api/orgs/{id}/deploys"), &Who::Session),
    )
    .await;

    assert_eq!(backwards, StatusCode::BAD_REQUEST);
    assert_eq!(body, "a window must end after it starts");
    assert_eq!(missing, StatusCode::BAD_REQUEST);
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn a_project_bound_key_is_narrowed_on_all_three_routes_and_cannot_ask_for_another() {
    let shelf = Arc::new(Shelf::default());
    let elsewhere = format!("&project={}", Uuid::from_u128(OTHER));

    for uri in [
        releases_of(MINE, ""),
        deploys_of(MINE, ""),
        one_of(MINE, "checkout", "2.14.1", ""),
    ] {
        let (status, _) = served(
            Some(Arc::clone(&shelf)),
            looking(&uri, &Who::Key(PROJECT_KEY)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{uri}");
    }
    for uri in [
        releases_of(MINE, &elsewhere),
        deploys_of(MINE, &elsewhere),
        one_of(MINE, "checkout", "2.14.1", &elsewhere),
    ] {
        let (status, _) = served(
            Some(Arc::clone(&shelf)),
            looking(&uri, &Who::Key(PROJECT_KEY)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }

    assert_eq!(
        shelf
            .asked()
            .into_iter()
            .map(|(_, project, _)| project)
            .collect::<Vec<_>>(),
        vec![Some(project_id(OURS)); 3]
    );
}

#[tokio::test]
async fn an_org_wide_key_reads_every_project_and_may_name_one() {
    let shelf = Arc::new(Shelf::default());

    let (whole, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&releases_of(MINE, ""), &Who::Key(ORG_KEY)),
    )
    .await;
    let (named, _) = served(
        Some(Arc::clone(&shelf)),
        looking(
            &releases_of(MINE, &format!("&project={}", Uuid::from_u128(OTHER))),
            &Who::Key(ORG_KEY),
        ),
    )
    .await;

    assert_eq!(whole, StatusCode::OK);
    assert_eq!(named, StatusCode::OK);
    assert_eq!(
        shelf
            .asked()
            .into_iter()
            .map(|(_, project, _)| project)
            .collect::<Vec<_>>(),
        vec![None, Some(project_id(OTHER))]
    );
}

#[tokio::test]
async fn a_stranger_and_an_anonymous_caller_are_refused() {
    let shelf = Arc::new(Shelf::default());

    let (theirs, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&releases_of(THEIRS, ""), &Who::Session),
    )
    .await;
    let (detail_theirs, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&one_of(THEIRS, "checkout", "1", ""), &Who::Session),
    )
    .await;
    let (key_elsewhere, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&deploys_of(THEIRS, ""), &Who::Key(ORG_KEY)),
    )
    .await;
    let (nobody, _) = served(
        Some(Arc::clone(&shelf)),
        looking(&releases_of(MINE, ""), &Who::Nobody),
    )
    .await;

    assert_eq!(theirs, StatusCode::NOT_FOUND);
    assert_eq!(detail_theirs, StatusCode::NOT_FOUND);
    assert_eq!(key_elsewhere, StatusCode::NOT_FOUND);
    assert_eq!(nobody, StatusCode::UNAUTHORIZED);
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn no_issue_store_and_a_broken_store_are_unavailable() {
    let (none, _) = served(None, looking(&releases_of(MINE, ""), &Who::Session)).await;
    let (broken, _) = served(
        Some(Arc::new(Shelf {
            broken: true,
            ..Shelf::default()
        })),
        looking(&releases_of(MINE, ""), &Who::Session),
    )
    .await;

    assert_eq!(none, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(broken, StatusCode::SERVICE_UNAVAILABLE);
}
