use core::marker::PhantomData;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, inbox_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, InboxItem, InboxItemDraft, InboxItemId, InboxItemKind, InboxStore, Membership,
    Memberships, NoPhotos, NotificationPreferences, NotificationSettings, OrgRole, Session,
    SessionError, SessionStore, SessionToken, StoreError, TenantId, UnixSeconds, WhoamiError,
};
use ferristrace_infra::FixedClock;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft.person.0123456789";
const ACCOUNT: u128 = 9;
const MINE: u128 = 1;
const OTHER: u128 = 2;
const NOW: u64 = 1_700_000_000;

fn id(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

fn account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

#[derive(Clone)]
struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
            Ok(Session {
                account: account(ACCOUNT),
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

#[derive(Clone)]
struct MemberOfMine;

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: org(MINE),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

#[derive(Debug, Default)]
struct Ledger {
    items: Mutex<Vec<InboxItem>>,
    prefs: Mutex<Option<NotificationPreferences>>,
}

impl Ledger {
    fn raise(
        &self,
        kind: InboxItemKind,
        account: AccountId,
        org: TenantId,
        at_ms: u64,
    ) -> InboxItem {
        let item = InboxItem {
            id: InboxItemId::fresh(),
            account,
            org,
            kind,
            title: "A new exception type".into(),
            body: "checkout-api started raising TimeoutError".into(),
            link: None,
            created_ms: at_ms,
            read_ms: None,
        };
        self.items.lock().expect("the lock").push(item.clone());
        item
    }
}

#[derive(Debug, Clone)]
struct Shared(Arc<Ledger>);

impl InboxStore for Shared {
    async fn list(
        &self,
        acc: AccountId,
        asked: TenantId,
        limit: u32,
    ) -> Result<Vec<InboxItem>, StoreError> {
        Ok(self
            .0
            .items
            .lock()
            .expect("the lock")
            .iter()
            .filter(|item| item.account == acc && item.org == asked)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn create(&self, draft: &InboxItemDraft, at_ms: u64) -> Result<InboxItem, StoreError> {
        Ok(self.0.raise(draft.kind, draft.account, draft.org, at_ms))
    }

    async fn mark_read(
        &self,
        acc: AccountId,
        asked: TenantId,
        id: InboxItemId,
        at_ms: u64,
    ) -> Result<bool, StoreError> {
        let mut items = self.0.items.lock().expect("the lock");
        let Some(item) = items
            .iter_mut()
            .find(|item| item.account == acc && item.org == asked && item.id == id)
        else {
            return Ok(false);
        };
        item.read_ms = Some(at_ms);
        Ok(true)
    }

    async fn mark_all_read(
        &self,
        acc: AccountId,
        asked: TenantId,
        at_ms: u64,
    ) -> Result<u64, StoreError> {
        let mut items = self.0.items.lock().expect("the lock");
        let mut flipped = 0;
        for item in items
            .iter_mut()
            .filter(|item| item.account == acc && item.org == asked && item.read_ms.is_none())
        {
            item.read_ms = Some(at_ms);
            flipped += 1;
        }
        Ok(flipped)
    }

    async fn prune(&self, _older_than_ms: u64) -> Result<u64, StoreError> {
        Ok(0)
    }
}

impl NotificationSettings for Shared {
    async fn of(&self, _account: AccountId) -> Result<Option<NotificationPreferences>, StoreError> {
        Ok(self.0.prefs.lock().expect("the lock").clone())
    }

    async fn set(
        &self,
        _account: AccountId,
        preferences: &NotificationPreferences,
    ) -> Result<(), StoreError> {
        *self.0.prefs.lock().expect("the lock") = Some(preferences.clone());
        Ok(())
    }
}

#[derive(Debug)]
struct Fakes {
    ledger: Arc<Ledger>,
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

repo!(Sessions, OneSession, |_fakes| OneSession);
repo!(Memberships, MemberOfMine, |_fakes| MemberOfMine);
repo!(Inbox, Shared, |fakes| Shared(Arc::clone(&fakes.ledger)));

type Unowned<T> = PhantomData<fn() -> T>;

#[derive(Debug)]
struct Wired(Unowned<()>);

impl Stack for Wired {
    type Repos = Fakes;
    type Clock = FixedClock;
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

fn app(ledger: Arc<Ledger>) -> Router {
    let adapters = Adapters {
        clock: FixedClock::at(NOW),
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
    inbox_router(AppState::new(Arc::new(Ferristrace::<Wired>::new(
        Fakes { ledger },
        adapters,
    ))))
}

fn as_session(method: Method, uri: &str, cookie: Option<&str>) -> Request<Body> {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        request = request.header(AUTHORIZATION, cookie);
    }
    request.body(Body::empty()).expect("a request")
}

fn mine() -> String {
    format!("Bearer {TOKEN}")
}

async fn served(ledger: Arc<Ledger>, request: Request<Body>) -> (StatusCode, Value) {
    let response = app(ledger).oneshot(request).await.expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()))
    };
    (status, value)
}

#[tokio::test]
async fn a_signed_in_account_lists_its_own_organisations_notifications() {
    let ledger = Arc::new(Ledger::default());
    ledger.raise(InboxItemKind::Issue, account(ACCOUNT), org(MINE), 1_000);
    ledger.raise(InboxItemKind::Usage, account(ACCOUNT), org(OTHER), 1_000);

    let (status, body) = served(
        Arc::clone(&ledger),
        as_session(
            Method::GET,
            &format!("/api/orgs/{}/notifications", id(MINE)),
            Some(&mine()),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let notifications = body["notifications"].as_array().expect("an array");
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0]["kind"], "issue");
}

#[tokio::test]
async fn listing_an_organisation_you_do_not_belong_to_is_not_found() {
    let ledger = Arc::new(Ledger::default());

    let (status, _) = served(
        ledger,
        as_session(
            Method::GET,
            &format!("/api/orgs/{}/notifications", id(OTHER)),
            Some(&mine()),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn without_a_session_listing_is_unauthorized() {
    let ledger = Arc::new(Ledger::default());

    let (status, _) = served(
        ledger,
        as_session(
            Method::GET,
            &format!("/api/orgs/{}/notifications", id(MINE)),
            None,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn marking_one_notification_read_leaves_the_rest_alone() {
    let ledger = Arc::new(Ledger::default());
    let raised = ledger.raise(InboxItemKind::Issue, account(ACCOUNT), org(MINE), 1_000);
    ledger.raise(InboxItemKind::Usage, account(ACCOUNT), org(MINE), 1_000);

    let (status, _) = served(
        Arc::clone(&ledger),
        as_session(
            Method::POST,
            &format!("/api/orgs/{}/notifications/{}/read", id(MINE), raised.id),
            Some(&mine()),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    let read: Vec<_> = ledger
        .items
        .lock()
        .expect("the lock")
        .iter()
        .filter(|item| item.read_ms.is_some())
        .map(|item| item.id)
        .collect();
    assert_eq!(read, vec![raised.id]);
}

#[tokio::test]
async fn marking_an_unknown_notification_read_is_not_found() {
    let ledger = Arc::new(Ledger::default());

    let (status, _) = served(
        ledger,
        as_session(
            Method::POST,
            &format!(
                "/api/orgs/{}/notifications/{}/read",
                id(MINE),
                InboxItemId::fresh()
            ),
            Some(&mine()),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mark_all_read_reports_how_many_it_changed() {
    let ledger = Arc::new(Ledger::default());
    ledger.raise(InboxItemKind::Issue, account(ACCOUNT), org(MINE), 1_000);
    ledger.raise(InboxItemKind::Usage, account(ACCOUNT), org(MINE), 1_000);

    let (status, body) = served(
        Arc::clone(&ledger),
        as_session(
            Method::POST,
            &format!("/api/orgs/{}/notifications/read-all", id(MINE)),
            Some(&mine()),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "marked": 2 }));
}
