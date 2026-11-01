use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::org_router;
use ferristrace_domain::{
    AccountId, Agent, Membership, Memberships, NoPhotos, OrgName, OrgRole, OrgSettings,
    Organisation, Orgs, Photo, PhotoError, PhotoKind, Photos, Pictured, Portrait, ReadError,
    Session, SessionError, SessionStore, SessionToken, TenantId, UnixSeconds, Whoami, WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ct_ss_0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const FOUNDED: u64 = 1_700_000_000_123_456_789;

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
struct Kept(Mutex<String>, Mutex<Option<Portrait>>);

impl Orgs for Kept {
    async fn only(&self, org: TenantId) -> Result<Option<Organisation>, ReadError> {
        Ok(Some(Organisation {
            id: org,
            name: self.0.lock().expect("the lock").clone().into(),
            plan: "Self-hosted".into(),
            created_at_unix_nanos: FOUNDED,
            logo: self.1.lock().expect("the lock").clone(),
        }))
    }

    async fn rename(&self, _org: TenantId, name: &OrgName) -> Result<(), ReadError> {
        let mut held = self.0.lock().expect("the lock");
        held.clear();
        held.push_str(name.as_str());
        Ok(())
    }

    async fn set_logo(&self, _org: TenantId, logo: Option<&Portrait>) -> Result<(), ReadError> {
        *self.1.lock().expect("the lock") = logo.cloned();
        Ok(())
    }
}

#[derive(Debug, Default)]
struct Shelf(Mutex<Vec<(String, Vec<u8>)>>);

impl Photos for Shelf {
    async fn put(&self, _of: Pictured, photo: &Photo) -> Result<Portrait, PhotoError> {
        let mut held = self.0.lock().expect("the lock");
        let token = format!("token{}", held.len() + 1);
        held.push((token.clone(), photo.bytes().to_vec()));
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
            .0
            .lock()
            .expect("the lock")
            .iter()
            .find(|(token, _)| token.as_str() == &*portrait.token)
            .map(|(_, bytes)| bytes.clone()))
    }

    async fn remove(&self, _of: Pictured, portrait: &Portrait) -> Result<(), PhotoError> {
        self.0
            .lock()
            .expect("the lock")
            .retain(|(token, _)| token.as_str() != &*portrait.token);
        Ok(())
    }
}

fn org(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

fn session() -> String {
    format!("ct_session={TOKEN}")
}

fn asking(method: Method, seed: u128, body: Option<&str>, scripted: bool) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("/api/orgs/{}", org(seed)))
        .header(COOKIE, session());
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

async fn served(role: OrgRole, kept: &Arc<Kept>, request: Request<Body>) -> (StatusCode, String) {
    let settings = Arc::new(OrgSettings::new(
        Whoami::new(OneSession, MemberOfMine(role)),
        Arc::clone(kept),
        NoPhotos,
    ));
    let response = org_router(settings)
        .oneshot(request)
        .await
        .expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8(body.to_vec()).expect("text"))
}

fn holding(name: &str) -> Arc<Kept> {
    Arc::new(Kept(Mutex::new(name.to_owned()), Mutex::default()))
}

fn a_png() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend_from_slice(b"and then whatever an encoder wrote");
    bytes
}

fn asking_logo(method: Method, seed: u128, body: Option<Vec<u8>>, scripted: bool) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("/api/orgs/{}/logo", org(seed)))
        .header(COOKIE, session());
    if scripted {
        request = request.header("x-requested-with", "fetch");
    }
    request
        .body(body.map_or_else(Body::empty, Body::from))
        .expect("a request")
}

async fn marked(
    role: OrgRole,
    kept: &Arc<Kept>,
    shelf: &Arc<Shelf>,
    request: Request<Body>,
) -> (StatusCode, Vec<u8>, Option<String>, Option<String>) {
    let settings = Arc::new(OrgSettings::new(
        Whoami::new(OneSession, MemberOfMine(role)),
        Arc::clone(kept),
        Arc::clone(shelf),
    ));
    let response = org_router(settings)
        .oneshot(request)
        .await
        .expect("a response");
    let status = response.status();
    let header = |name: &axum::http::HeaderName| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let kind = header(&CONTENT_TYPE);
    let caching = header(&CACHE_CONTROL);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, body.to_vec(), kind, caching)
}

fn renaming(name: &str) -> String {
    serde_json::json!({ "name": name }).to_string()
}

#[tokio::test]
async fn a_member_reads_the_card_of_its_own_organisation() {
    let kept = holding("Acme");

    let (status, body) = served(
        OrgRole::Member,
        &kept,
        asking(Method::GET, MINE, None, true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let card: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(card["id"], org(MINE));
    assert_eq!(card["name"], "Acme");
    assert_eq!(card["plan"], "Self-hosted");
    assert_eq!(card["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        card["created_at_ms"], 1_700_000_000_123u64,
        "a time a browser can hold without losing digits"
    );
}

#[tokio::test]
async fn a_request_from_another_origin_never_reaches_the_organisation() {
    let kept = holding("Acme");

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(Method::GET, MINE, None, false),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_organisation_this_session_does_not_belong_to_is_not_found() {
    let kept = holding("Acme");

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking(Method::GET, THEIRS, None, true),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
}

#[tokio::test]
async fn an_administrator_renames_it_and_is_answered_with_the_card() {
    let kept = holding("Acme");

    let (status, body) = served(
        OrgRole::Admin,
        &kept,
        asking(Method::PATCH, MINE, Some(&renaming("Acme Payments")), true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let card: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(card["name"], "Acme Payments");
    assert_eq!(*kept.0.lock().expect("the lock"), "Acme Payments");
}

#[tokio::test]
async fn a_member_is_told_plainly_that_renaming_is_not_its_to_do() {
    let kept = holding("Acme");

    let (status, _) = served(
        OrgRole::Member,
        &kept,
        asking(Method::PATCH, MINE, Some(&renaming("Mine Now")), true),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(*kept.0.lock().expect("the lock"), "Acme");
}

#[tokio::test]
async fn a_blank_name_is_refused_with_the_rule_that_refused_it() {
    let kept = holding("Acme");

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(Method::PATCH, MINE, Some(&renaming("   ")), true),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("needs a name"), "{body}");
    assert_eq!(*kept.0.lock().expect("the lock"), "Acme");
}

#[tokio::test]
async fn a_name_is_kept_without_the_space_around_it() {
    let kept = holding("Acme");

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking(Method::PATCH, MINE, Some(&renaming("  Acme  ")), true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let card: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(card["name"], "Acme");
}

#[tokio::test]
async fn an_administrator_keeps_a_logo_and_the_card_carries_its_url() {
    let kept = holding("Acme");
    let shelf = Arc::new(Shelf::default());

    let (status, body, _, _) = marked(
        OrgRole::Admin,
        &kept,
        &shelf,
        asking_logo(Method::POST, MINE, Some(a_png()), true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let card: serde_json::Value =
        serde_json::from_str(&String::from_utf8(body).expect("text")).expect("json");
    assert_eq!(
        card["logo"],
        format!("/api/orgs/{}/logo?v=token1", org(MINE))
    );
    assert_eq!(shelf.0.lock().expect("the lock").len(), 1);
}

#[tokio::test]
async fn a_member_may_not_keep_a_logo() {
    let kept = holding("Acme");
    let shelf = Arc::new(Shelf::default());

    let (status, _, _, _) = marked(
        OrgRole::Member,
        &kept,
        &shelf,
        asking_logo(Method::POST, MINE, Some(a_png()), true),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        shelf.0.lock().expect("the lock").is_empty(),
        "nothing reached the store"
    );
}

#[tokio::test]
async fn a_script_announced_as_an_image_is_refused_with_the_rule_that_refused_it() {
    let kept = holding("Acme");
    let shelf = Arc::new(Shelf::default());

    let (status, body, _, _) = marked(
        OrgRole::Owner,
        &kept,
        &shelf,
        asking_logo(
            Method::POST,
            MINE,
            Some(b"<script>alert(1)</script>".to_vec()),
            true,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let said = String::from_utf8(body).expect("text");
    assert!(said.contains("png"), "{said}");
    assert!(shelf.0.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn an_image_element_fetches_the_logo_without_the_header_a_form_must_carry() {
    let kept = holding("Acme");
    let shelf = Arc::new(Shelf::default());
    marked(
        OrgRole::Admin,
        &kept,
        &shelf,
        asking_logo(Method::POST, MINE, Some(a_png()), true),
    )
    .await;

    let (status, body, kind, caching) = marked(
        OrgRole::Member,
        &kept,
        &shelf,
        asking_logo(Method::GET, MINE, None, false),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, a_png());
    assert_eq!(kind.as_deref(), Some(PhotoKind::Png.as_str()));
    assert_eq!(
        caching.as_deref(),
        Some("private, max-age=31536000, immutable"),
        "the token in the url is what makes a year safe"
    );
}

#[tokio::test]
async fn an_organisation_that_keeps_no_logo_answers_that_there_is_none() {
    let kept = holding("Acme");
    let shelf = Arc::new(Shelf::default());

    let (status, _, _, _) = marked(
        OrgRole::Owner,
        &kept,
        &shelf,
        asking_logo(Method::GET, MINE, None, false),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_stranger_is_not_told_whether_another_organisation_keeps_a_logo() {
    let kept = holding("Acme");
    let shelf = Arc::new(Shelf::default());

    let (status, _, _, _) = marked(
        OrgRole::Owner,
        &kept,
        &shelf,
        asking_logo(Method::GET, THEIRS, None, false),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
}

#[tokio::test]
async fn a_logo_is_dropped_and_the_card_comes_back_without_one() {
    let kept = holding("Acme");
    let shelf = Arc::new(Shelf::default());
    marked(
        OrgRole::Admin,
        &kept,
        &shelf,
        asking_logo(Method::POST, MINE, Some(a_png()), true),
    )
    .await;

    let (status, body, _, _) = marked(
        OrgRole::Admin,
        &kept,
        &shelf,
        asking_logo(Method::DELETE, MINE, None, true),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let card: serde_json::Value =
        serde_json::from_str(&String::from_utf8(body).expect("text")).expect("json");
    assert!(card["logo"].is_null());
    assert!(
        shelf.0.lock().expect("the lock").is_empty(),
        "the object is forgotten, not just unlinked"
    );
}

#[tokio::test]
async fn a_deployment_with_no_object_store_says_which_part_is_missing() {
    let kept = holding("Acme");

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking_logo(Method::POST, MINE, Some(a_png()), true),
    )
    .await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("object store"), "{body}");
}
