use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, view_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, Creation, ExplorerViews, Membership, Memberships, NewView, NoPhotos, OrgRole,
    Saving, Session, SessionError, SessionStore, SessionToken, StoreError, TenantId, UnixSeconds,
    View, ViewChange, ViewId, ViewSignal, Visibility, WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft.person.0123456789";
const MINE: u128 = 1;
const ME: u128 = 9;
const SOMEONE: u128 = 10;
const SHARED: u128 = 100;
const THEIRS_PRIVATE: u128 = 101;
const STAMP: u64 = 1_700_000_000_123_456_789;

fn id(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
            Ok(Session {
                account: AccountId::new(Uuid::from_u128(ME)),
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

fn a_view(seed: u128, name: &str, visibility: Visibility, by: u128) -> View {
    View {
        id: ViewId::new(Uuid::from_u128(seed)),
        signal: ViewSignal::Traces,
        name: name.into(),
        visibility,
        definition: r#"{"q":[]}"#.into(),
        created_by: Some(AccountId::new(Uuid::from_u128(by))),
        created_at_unix_nanos: STAMP,
        updated_at_unix_nanos: STAMP,
    }
}

#[derive(Debug)]
struct Kept {
    views: Mutex<Vec<View>>,
}

impl Kept {
    fn with_two() -> Arc<Self> {
        Arc::new(Self {
            views: Mutex::new(vec![
                a_view(SHARED, "Slow checkout", Visibility::Org, SOMEONE),
                a_view(THEIRS_PRIVATE, "Someone's", Visibility::Private, SOMEONE),
            ]),
        })
    }
}

impl ExplorerViews for Kept {
    async fn visible(
        &self,
        _org: TenantId,
        _signal: ViewSignal,
        to: AccountId,
    ) -> Result<Vec<View>, StoreError> {
        Ok(self
            .views
            .lock()
            .expect("the lock")
            .iter()
            .filter(|view| view.visibility == Visibility::Org || view.created_by == Some(to))
            .cloned()
            .collect())
    }

    async fn find(&self, _org: TenantId, id: ViewId) -> Result<Option<View>, StoreError> {
        Ok(self
            .views
            .lock()
            .expect("the lock")
            .iter()
            .find(|view| view.id == id)
            .cloned())
    }

    async fn create(
        &self,
        _org: TenantId,
        by: AccountId,
        view: &NewView,
    ) -> Result<Creation, StoreError> {
        let mut views = self.views.lock().expect("the lock");
        if views.iter().any(|held| &*held.name == view.name.as_str()) {
            return Ok(Creation::NameTaken);
        }
        let made = View {
            id: ViewId::new(Uuid::from_u128(200 + views.len() as u128)),
            signal: view.signal,
            name: view.name.as_str().into(),
            visibility: view.visibility,
            definition: view.definition.as_str().into(),
            created_by: Some(by),
            created_at_unix_nanos: STAMP,
            updated_at_unix_nanos: STAMP,
        };
        views.push(made.clone());
        Ok(Creation::Made(made))
    }

    async fn change(
        &self,
        _org: TenantId,
        id: ViewId,
        change: &ViewChange,
    ) -> Result<Saving, StoreError> {
        let mut views = self.views.lock().expect("the lock");
        let Some(view) = views.iter_mut().find(|view| view.id == id) else {
            return Ok(Saving::Missing);
        };
        view.name = change.name.as_str().into();
        view.definition = change.definition.as_str().into();
        if let Some(visibility) = change.visibility {
            view.visibility = visibility;
        }
        Ok(Saving::Saved)
    }

    async fn delete(&self, _org: TenantId, id: ViewId) -> Result<bool, StoreError> {
        let mut views = self.views.lock().expect("the lock");
        let before = views.len();
        views.retain(|view| view.id != id);
        Ok(views.len() < before)
    }
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

impl RepoFor<domain::Views> for Fakes {
    type Repo = Arc<Kept>;

    fn build(&self, (): &()) -> Self::Repo {
        Arc::clone(&self.kept)
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

fn state(role: OrgRole, kept: &Arc<Kept>) -> AppState<TestStack> {
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
    let fakes = Fakes {
        role,
        kept: Arc::clone(kept),
    };
    AppState::new(Arc::new(Ferristrace::new(fakes, adapters)))
}

fn asking(method: Method, uri: &str, bearer: Option<&str>, body: Option<&str>) -> Request<Body> {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(bearer) = bearer {
        request = request.header(AUTHORIZATION, format!("Bearer {bearer}"));
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    request
        .body(body.map_or_else(Body::empty, |json| Body::from(json.to_owned())))
        .expect("a request")
}

async fn served(role: OrgRole, kept: &Arc<Kept>, request: Request<Body>) -> (StatusCode, String) {
    let response = view_router(state(role, kept))
        .oneshot(request)
        .await
        .expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("text"))
}

async fn as_me(
    role: OrgRole,
    kept: &Arc<Kept>,
    method: Method,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, String) {
    served(role, kept, asking(method, uri, Some(TOKEN), body)).await
}

fn read(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("json")
}

fn views_of(org: u128) -> String {
    format!("/api/orgs/{}/views", id(org))
}

fn view_of(org: u128, view: u128) -> String {
    format!("/api/orgs/{}/views/{}", id(org), id(view))
}

#[tokio::test]
async fn a_member_lists_the_organisation_views_and_not_someone_elses_private_one() {
    let kept = Kept::with_two();

    let (status, body) = as_me(OrgRole::Member, &kept, Method::GET, &views_of(MINE), None).await;

    assert_eq!(status, StatusCode::OK);
    let listed = &read(&body)["views"];
    assert_eq!(listed.as_array().expect("a list").len(), 1);
    assert_eq!(listed[0]["name"], "Slow checkout");
    assert_eq!(listed[0]["visibility"], "org");
    assert_eq!(listed[0]["definition"]["q"], serde_json::json!([]));
    assert_eq!(listed[0]["created_by"], id(SOMEONE));
    assert_eq!(listed[0]["updated_at_ms"], 1_700_000_000_123u64);
}

#[tokio::test]
async fn a_member_saves_a_view_and_gets_it_back_with_its_definition() {
    let kept = Kept::with_two();
    let body = r#"{"name":" Errors on checkout ","visibility":"private",
        "definition":{"q":[{"field":{"built":"status"},"op":"eq","value":"error"}],"order":"started"}}"#;

    let (status, made) = as_me(
        OrgRole::Member,
        &kept,
        Method::POST,
        &views_of(MINE),
        Some(body),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let view = read(&made);
    assert_eq!(view["name"], "Errors on checkout");
    assert_eq!(view["visibility"], "private");
    assert_eq!(view["signal"], "traces");
    assert_eq!(view["created_by"], id(ME));
    assert_eq!(view["definition"]["order"], "started");
}

#[tokio::test]
async fn a_view_the_domain_refuses_is_unprocessable() {
    let kept = Kept::with_two();
    let refused = [
        r#"{"name":"","visibility":"org","definition":{}}"#,
        r#"{"name":"x","visibility":"public","definition":{}}"#,
        r#"{"name":"x","visibility":"org","definition":[1]}"#,
        r#"{"signal":"logs","name":"x","visibility":"org","definition":{}}"#,
    ];

    for body in refused {
        let (status, _) = as_me(
            OrgRole::Member,
            &kept,
            Method::POST,
            &views_of(MINE),
            Some(body),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    assert_eq!(kept.views.lock().expect("the lock").len(), 2);
}

#[tokio::test]
async fn a_name_already_in_use_is_a_conflict() {
    let kept = Kept::with_two();
    let body = r#"{"name":"Slow checkout","visibility":"org","definition":{}}"#;

    let (status, _) = as_me(
        OrgRole::Member,
        &kept,
        Method::POST,
        &views_of(MINE),
        Some(body),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_member_cannot_change_or_delete_someone_elses_organisation_view_but_an_admin_can() {
    let kept = Kept::with_two();
    let body = r#"{"name":"Renamed","definition":{"q":[]}}"#;

    let (edited, _) = as_me(
        OrgRole::Member,
        &kept,
        Method::PUT,
        &view_of(MINE, SHARED),
        Some(body),
    )
    .await;
    let (deleted, _) = as_me(
        OrgRole::Member,
        &kept,
        Method::DELETE,
        &view_of(MINE, SHARED),
        None,
    )
    .await;
    let (by_admin, changed) = as_me(
        OrgRole::Admin,
        &kept,
        Method::PUT,
        &view_of(MINE, SHARED),
        Some(body),
    )
    .await;
    let (removed, _) = as_me(
        OrgRole::Admin,
        &kept,
        Method::DELETE,
        &view_of(MINE, SHARED),
        None,
    )
    .await;

    assert_eq!(edited, StatusCode::FORBIDDEN);
    assert_eq!(deleted, StatusCode::FORBIDDEN);
    assert_eq!(by_admin, StatusCode::OK);
    assert_eq!(read(&changed)["name"], "Renamed");
    assert_eq!(removed, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn someone_elses_private_view_is_not_found_even_for_an_owner() {
    let kept = Kept::with_two();

    let (status, _) = as_me(
        OrgRole::Owner,
        &kept,
        Method::DELETE,
        &view_of(MINE, THEIRS_PRIVATE),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(kept.views.lock().expect("the lock").len(), 2);
}

#[tokio::test]
async fn only_the_creator_changes_who_sees_a_view() {
    let kept = Kept::with_two();
    let body = r#"{"name":"Slow checkout","visibility":"private","definition":{}}"#;

    let (status, _) = as_me(
        OrgRole::Admin,
        &kept,
        Method::PUT,
        &view_of(MINE, SHARED),
        Some(body),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_organisation_this_session_does_not_belong_to_is_not_found() {
    let kept = Kept::with_two();

    let (status, _) = as_me(OrgRole::Member, &kept, Method::GET, &views_of(2), None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_request_without_a_person_session_is_refused() {
    let kept = Kept::with_two();
    for bearer in [
        None,
        Some("ft_live_an_api_key"),
        Some("ft.agent.0123456789"),
    ] {
        let (status, _) = served(
            OrgRole::Owner,
            &kept,
            asking(Method::GET, &views_of(MINE), bearer, None),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED, "{bearer:?}");
    }
}
