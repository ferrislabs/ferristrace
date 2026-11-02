use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::SessionCookie;
use ferristrace_api::me_router;
use ferristrace_application::testing::Inert;
use ferristrace_domain::{
    AccountId, AccountName, Accounts, Agent, MAX_PHOTO_BYTES, Membership, Memberships, NoPhotos,
    OrgRole, Photo, PhotoError, PhotoKind, Photos, Pictured, Portrait, Session, SessionError,
    SessionStore, SessionToken, StoreError, TenantId, UnixSeconds, WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

#[allow(dead_code)]
#[path = "support/identity.rs"]
mod identity;

use identity::{Fakes, Nowhere, Profiled, state};

const TOKEN: &str = "ft_ss_0123456789";
const ACCOUNT: u128 = 7;
const ACTIVE: u128 = 11;
const OTHER: u128 = 12;
const SOMEBODY_ELSE: u128 = 13;
const MAX_NAME_BYTES: usize = 128;

#[derive(Debug, Default)]
struct Kept {
    name: Mutex<Option<String>>,
    portrait: Mutex<Option<Portrait>>,
    writes: Mutex<Vec<Option<String>>>,
}

impl Kept {
    fn named(name: &str) -> Self {
        Self {
            name: Mutex::new(Some(name.to_owned())),
            ..Self::default()
        }
    }

    fn writes(&self) -> Vec<Option<String>> {
        self.writes.lock().expect("the lock").clone()
    }

    fn portrait(&self) -> Option<Portrait> {
        self.portrait.lock().expect("the lock").clone()
    }
}

impl Accounts for Kept {
    async fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> Result<(), StoreError> {
        assert_eq!(
            account,
            AccountId::new(Uuid::from_u128(ACCOUNT)),
            "a rename writes to the account the session resolved to"
        );
        let chosen = name.map(|name| name.as_str().to_owned());
        self.writes.lock().expect("the lock").push(chosen.clone());
        *self.name.lock().expect("the lock") = chosen;
        Ok(())
    }

    async fn portrait_of(&self, account: AccountId) -> Result<Option<Portrait>, StoreError> {
        if account != AccountId::new(Uuid::from_u128(ACCOUNT)) {
            return Ok(None);
        }
        Ok(self.portrait())
    }

    async fn set_portrait(
        &self,
        _account: AccountId,
        portrait: Option<&Portrait>,
    ) -> Result<(), StoreError> {
        *self.portrait.lock().expect("the lock") = portrait.cloned();
        Ok(())
    }
}

#[derive(Debug, Default)]
struct Shelf {
    objects: Mutex<Vec<(String, Vec<u8>)>>,
}

impl Photos for Shelf {
    async fn put(&self, _of: Pictured, photo: &Photo) -> Result<Portrait, PhotoError> {
        let token = format!("token{}", self.objects.lock().expect("the lock").len() + 1);
        self.objects
            .lock()
            .expect("the lock")
            .push((token.clone(), photo.bytes().to_vec()));
        Ok(Portrait {
            token: token.into(),
            kind: photo.kind(),
        })
    }

    async fn fetch(
        &self,
        _of: Pictured,
        portrait: &Portrait,
    ) -> Result<Option<Vec<u8>>, PhotoError> {
        Ok(self
            .objects
            .lock()
            .expect("the lock")
            .iter()
            .find(|(token, _)| token.as_str() == &*portrait.token)
            .map(|(_, bytes)| bytes.clone()))
    }

    async fn remove(&self, _of: Pictured, portrait: &Portrait) -> Result<(), PhotoError> {
        self.objects
            .lock()
            .expect("the lock")
            .retain(|(token, _)| token.as_str() != &*portrait.token);
        Ok(())
    }
}

#[derive(Clone)]
struct OneSession {
    org: Option<TenantId>,
    kept: Arc<Kept>,
}

impl SessionStore for OneSession {
    async fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        let _ = (account, org, from);
        Ok(SessionToken::from_plaintext(TOKEN))
    }

    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.exposed() == TOKEN {
            Ok(Session {
                account: AccountId::new(Uuid::from_u128(ACCOUNT)),
                email: an_email(),
                name: self
                    .kept
                    .name
                    .lock()
                    .expect("the lock")
                    .clone()
                    .map(Into::into),
                photo: self.kept.portrait(),
                org: self.org,
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

#[derive(Clone)]
struct TwoOrgs;

impl Memberships for TwoOrgs {
    async fn of(&self, account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        if account != AccountId::new(Uuid::from_u128(ACCOUNT)) {
            return Ok(Vec::new());
        }
        Ok(vec![
            Membership {
                org: TenantId::new(Uuid::from_u128(ACTIVE)),
                name: "Acme".into(),
                role: OrgRole::Owner,
            },
            Membership {
                org: TenantId::new(Uuid::from_u128(OTHER)),
                name: "Globex".into(),
                role: OrgRole::Member,
            },
        ])
    }
}

#[derive(Clone)]
struct NoControlPlane;

impl Memberships for NoControlPlane {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Err(WhoamiError::StoreUnreachable)
    }
}

fn a_png() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend_from_slice(b"a face");
    bytes
}

fn asking(cookie: Option<&str>, scripted: bool) -> Request<Body> {
    let mut request = Request::builder().uri("/auth/me");
    if scripted {
        request = request.header("x-requested-with", "fetch");
    }
    if let Some(cookie) = cookie {
        request = request.header(COOKIE, cookie);
    }
    request.body(Body::empty()).expect("a request")
}

fn naming(spelling: &str, cookie: Option<&str>, scripted: bool) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::PATCH)
        .uri("/auth/me")
        .header(CONTENT_TYPE, "application/json");
    if scripted {
        request = request.header("x-requested-with", "fetch");
    }
    if let Some(cookie) = cookie {
        request = request.header(COOKIE, cookie);
    }
    let body = serde_json::json!({ "name": spelling }).to_string();
    request.body(Body::from(body)).expect("a request")
}

fn offering(bytes: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/auth/me/photo")
        .header("x-requested-with", "fetch")
        .header(COOKIE, mine())
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(bytes))
        .expect("a request")
}

fn dropping() -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri("/auth/me/photo")
        .header("x-requested-with", "fetch")
        .header(COOKIE, mine())
        .body(Body::empty())
        .expect("a request")
}

fn looking(at: u128, cookie: Option<&str>) -> Request<Body> {
    let account = Uuid::from_u128(at);
    let mut request = Request::builder().uri(format!("/auth/accounts/{account}/photo?v=token1"));
    if let Some(cookie) = cookie {
        request = request.header(COOKIE, cookie);
    }
    request.body(Body::empty()).expect("a request")
}

fn mine() -> String {
    format!("ft_session={TOKEN}")
}

async fn answered<G, P>(
    kept: &Arc<Kept>,
    photos: P,
    memberships: G,
    request: Request<Body>,
) -> (StatusCode, Vec<u8>, Option<String>, Option<String>)
where
    G: Memberships + Clone + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
{
    let fakes = Fakes {
        sessions: OneSession {
            org: Some(TenantId::new(Uuid::from_u128(ACTIVE))),
            kept: Arc::clone(kept),
        },
        memberships,
        accounts: Profiled(Arc::clone(kept)),
        flows: Nowhere,
    };
    let me = state(
        fakes,
        Inert::default(),
        photos,
        None::<Inert>,
        SessionCookie::default(),
    );
    let response = me_router(me).oneshot(request).await.expect("a response");
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let read = |name| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    (
        status,
        body.to_vec(),
        read(&CONTENT_TYPE),
        read(&CACHE_CONTROL),
    )
}

async fn served<G>(kept: &Arc<Kept>, memberships: G, request: Request<Body>) -> (StatusCode, String)
where
    G: Memberships + Clone + Send + Sync + 'static,
{
    let (status, body, _, _) = answered(kept, Shelf::default(), memberships, request).await;
    (status, String::from_utf8(body).expect("text"))
}

async fn with_shelf(
    kept: &Arc<Kept>,
    shelf: &Arc<Shelf>,
    request: Request<Body>,
) -> (StatusCode, Vec<u8>, Option<String>, Option<String>) {
    answered(kept, Arc::clone(shelf), TwoOrgs, request).await
}

fn standing_in(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("json")
}

#[tokio::test]
async fn a_live_session_is_answered_with_its_account_and_its_roles() {
    let kept = Arc::new(Kept::default());

    let (status, body) = served(&kept, TwoOrgs, asking(Some(&mine()), true)).await;

    assert_eq!(status, StatusCode::OK);
    let standing = standing_in(&body);
    assert_eq!(
        standing["account"],
        serde_json::json!(Uuid::from_u128(ACCOUNT).to_string())
    );
    assert_eq!(
        standing["active_org"],
        serde_json::json!(Uuid::from_u128(ACTIVE).to_string())
    );
    assert_eq!(
        standing["email"], "person@example.com",
        "a menu has an address to show rather than an identifier"
    );
    assert_eq!(
        standing["name"],
        serde_json::Value::Null,
        "an account nobody has named carries no name rather than an empty one"
    );
    assert_eq!(
        standing["photo"],
        serde_json::Value::Null,
        "and no photo rather than a url that answers nothing"
    );
    assert_eq!(
        standing["memberships"],
        serde_json::json!([
            {
                "org": Uuid::from_u128(ACTIVE).to_string(),
                "name": "Acme",
                "role": "owner",
            },
            {
                "org": Uuid::from_u128(OTHER).to_string(),
                "name": "Globex",
                "role": "member",
            },
        ])
    );
}

#[tokio::test]
async fn an_account_that_carries_a_name_is_answered_with_it() {
    let kept = Arc::new(Kept::named("Ines Martin"));

    let (status, body) = served(&kept, TwoOrgs, asking(Some(&mine()), true)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(standing_in(&body)["name"], "Ines Martin");
}

#[tokio::test]
async fn a_request_without_a_session_cookie_is_refused() {
    let kept = Arc::new(Kept::default());

    let (status, _) = served(&kept, TwoOrgs, asking(None, true)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_cookie_the_store_does_not_know_is_refused() {
    let kept = Arc::new(Kept::default());

    let (status, _) = served(&kept, TwoOrgs, asking(Some("ft_session=ft_ss_nope"), true)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_request_that_no_script_sent_is_refused() {
    let kept = Arc::new(Kept::default());

    let (status, _) = served(&kept, TwoOrgs, asking(Some(&mine()), false)).await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the standing must not be readable by a plain cross site navigation"
    );
}

#[tokio::test]
async fn an_unreachable_control_plane_is_not_reported_as_a_missing_session() {
    let kept = Arc::new(Kept::default());

    let (status, _) = served(&kept, NoControlPlane, asking(Some(&mine()), true)).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn a_chosen_name_is_written_and_the_whole_standing_comes_back() {
    let kept = Arc::new(Kept::default());

    let (status, body) = served(
        &kept,
        TwoOrgs,
        naming("  Ines Martin  ", Some(&mine()), true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let standing = standing_in(&body);
    assert_eq!(
        standing["name"], "Ines Martin",
        "the trimming the name went through is what the screen is told"
    );
    assert_eq!(
        standing["memberships"].as_array().expect("an array").len(),
        2,
        "the screen beside the field shows the organisations, so the answer carries them"
    );
    assert_eq!(kept.writes(), vec![Some("Ines Martin".to_owned())]);
}

#[tokio::test]
async fn an_emptied_name_is_taken_off_the_account() {
    let kept = Arc::new(Kept::named("Ines Martin"));

    let (status, body) = served(&kept, TwoOrgs, naming("", Some(&mine()), true)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(standing_in(&body)["name"], serde_json::Value::Null);
    assert_eq!(kept.writes(), vec![None]);
}

#[tokio::test]
async fn a_name_of_nothing_but_space_is_taken_off_too() {
    let kept = Arc::new(Kept::named("Ines Martin"));

    let (status, _) = served(&kept, TwoOrgs, naming("   \t ", Some(&mine()), true)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(kept.writes(), vec![None]);
}

#[tokio::test]
async fn a_name_past_the_ceiling_is_refused_and_nothing_is_written() {
    let kept = Arc::new(Kept::default());
    let long = "a".repeat(MAX_NAME_BYTES + 1);

    let (status, body) = served(&kept, TwoOrgs, naming(&long, Some(&mine()), true)).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body.contains(&MAX_NAME_BYTES.to_string()),
        "the answer says what the rule is: {body}"
    );
    assert!(kept.writes().is_empty());
}

#[tokio::test]
async fn renaming_without_a_session_writes_nothing() {
    let kept = Arc::new(Kept::default());

    let (status, _) = served(&kept, TwoOrgs, naming("Somebody Else", None, true)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(kept.writes().is_empty());
}

#[tokio::test]
async fn a_rename_no_script_sent_is_refused() {
    let kept = Arc::new(Kept::named("Ines Martin"));

    let (status, _) = served(
        &kept,
        TwoOrgs,
        naming("Somebody Else", Some(&mine()), false),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a cross site form must not be able to rename somebody"
    );
    assert!(kept.writes().is_empty());
}

#[tokio::test]
async fn a_photo_is_kept_and_the_standing_points_at_a_url_that_carries_its_token() {
    let (kept, shelf) = (Arc::new(Kept::default()), Arc::new(Shelf::default()));

    let (status, body, _, _) = with_shelf(&kept, &shelf, offering(a_png())).await;

    assert_eq!(status, StatusCode::OK);
    let standing = standing_in(&String::from_utf8(body).expect("text"));
    let account = Uuid::from_u128(ACCOUNT);
    assert_eq!(
        standing["photo"],
        serde_json::json!(format!("/auth/accounts/{account}/photo?v=token1")),
        "the token is in the url, so a replacement is a different url"
    );
    assert_eq!(kept.portrait().expect("a portrait").kind, PhotoKind::Png);
}

#[tokio::test]
async fn a_photo_that_is_not_an_image_is_refused_and_nothing_is_kept() {
    let (kept, shelf) = (Arc::new(Kept::default()), Arc::new(Shelf::default()));

    let (status, body, _, _) = with_shelf(
        &kept,
        &shelf,
        offering(b"<script>alert(1)</script>".to_vec()),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        String::from_utf8_lossy(&body).contains("png"),
        "the answer says what is admitted"
    );
    assert_eq!(kept.portrait(), None);
}

#[tokio::test]
async fn a_photo_past_the_ceiling_never_reaches_the_handler() {
    let (kept, shelf) = (Arc::new(Kept::default()), Arc::new(Shelf::default()));
    let mut huge = a_png();
    huge.resize(MAX_PHOTO_BYTES + 1, 0);

    let (status, _, _, _) = with_shelf(&kept, &shelf, offering(huge)).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(kept.portrait(), None);
}

#[tokio::test]
async fn a_kept_photo_is_served_with_what_it_is_and_a_year_of_cache() {
    let (kept, shelf) = (Arc::new(Kept::default()), Arc::new(Shelf::default()));
    with_shelf(&kept, &shelf, offering(a_png())).await;

    let (status, body, kind, cache) =
        with_shelf(&kept, &shelf, looking(ACCOUNT, Some(&mine()))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, a_png());
    assert_eq!(kind.as_deref(), Some("image/png"));
    let cache = cache.expect("a cache header");
    assert!(cache.contains("immutable"), "{cache}");
    assert!(cache.contains("private"), "{cache}");
}

#[tokio::test]
async fn a_photo_is_served_without_the_header_but_never_without_a_session() {
    let (kept, shelf) = (Arc::new(Kept::default()), Arc::new(Shelf::default()));
    with_shelf(&kept, &shelf, offering(a_png())).await;

    let (with, _, _, _) = with_shelf(&kept, &shelf, looking(ACCOUNT, Some(&mine()))).await;
    let (without, _, _, _) = with_shelf(&kept, &shelf, looking(ACCOUNT, None)).await;

    assert_eq!(with, StatusCode::OK, "an <img src> carries no header");
    assert_eq!(without, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_account_that_keeps_no_photo_is_answered_not_found() {
    let (kept, shelf) = (Arc::new(Kept::default()), Arc::new(Shelf::default()));
    with_shelf(&kept, &shelf, offering(a_png())).await;

    let (status, _, _, _) = with_shelf(&kept, &shelf, looking(SOMEBODY_ELSE, Some(&mine()))).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_dropped_photo_leaves_the_standing_without_one() {
    let (kept, shelf) = (Arc::new(Kept::default()), Arc::new(Shelf::default()));
    with_shelf(&kept, &shelf, offering(a_png())).await;

    let (status, body, _, _) = with_shelf(&kept, &shelf, dropping()).await;

    assert_eq!(status, StatusCode::OK);
    let standing = standing_in(&String::from_utf8(body).expect("text"));
    assert_eq!(standing["photo"], serde_json::Value::Null);
    assert_eq!(kept.portrait(), None);
    assert!(
        shelf.objects.lock().expect("the lock").is_empty(),
        "the bytes go with it"
    );
}

#[tokio::test]
async fn a_deployment_with_no_object_store_says_so() {
    let kept = Arc::new(Kept::default());

    let (status, body, _, _) = answered(&kept, NoPhotos, TwoOrgs, offering(a_png())).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        String::from_utf8_lossy(&body).contains("object store"),
        "the answer names the missing part"
    );
    assert_eq!(kept.portrait(), None);
}

fn an_email() -> ferristrace_domain::Email {
    ferristrace_domain::Email::try_from("person@example.com").expect("an address")
}
