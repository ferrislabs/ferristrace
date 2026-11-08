use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{AppState, org_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, ApiKey, ApiKeys, ApiSecret, IssuedKey, KeyId, KeyName, KeyScopes, KeyTtl,
    Membership, Memberships, NoPhotos, OrgName, OrgRole, OrgTimezone, Organisation, Orgs, Photo,
    PhotoError, PhotoKind, Photos, Pictured, Portrait, ProjectId, ReadGrant, RetentionDays,
    Session, SessionError, SessionStore, SessionToken, StoreError, TenantId, UnixSeconds,
    WhoamiError,
};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "ft.person.0123456789";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const FOUNDED: u64 = 1_700_000_000_123_456_789;
const READ_KEY: &str = "ft_live_abcdef012345.secret";

struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
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
struct Kept(
    Mutex<String>,
    Mutex<Option<Portrait>>,
    Mutex<Option<RetentionDays>>,
    Option<RetentionDays>,
    Mutex<OrgTimezone>,
    Mutex<bool>,
);

impl Orgs for Kept {
    async fn only(&self, org: TenantId) -> Result<Option<Organisation>, StoreError> {
        if *self.5.lock().expect("the lock") {
            return Ok(None);
        }
        Ok(Some(Organisation {
            id: org,
            name: self.0.lock().expect("the lock").clone().into(),
            plan: "Self-hosted".into(),
            created_at_unix_nanos: FOUNDED,
            logo: self.1.lock().expect("the lock").clone(),
            retention: *self.2.lock().expect("the lock"),
            retention_cap: self.3,
            timezone: self.4.lock().expect("the lock").clone(),
        }))
    }

    async fn rename(&self, _org: TenantId, name: &OrgName) -> Result<(), StoreError> {
        let mut held = self.0.lock().expect("the lock");
        held.clear();
        held.push_str(name.as_str());
        Ok(())
    }

    async fn disband(&self, _org: TenantId) -> Result<bool, StoreError> {
        let mut gone = self.5.lock().expect("the lock");
        let held = !*gone;
        *gone = true;
        Ok(held)
    }

    async fn set_logo(&self, _org: TenantId, logo: Option<&Portrait>) -> Result<(), StoreError> {
        *self.1.lock().expect("the lock") = logo.cloned();
        Ok(())
    }

    async fn set_retention(
        &self,
        _org: TenantId,
        retention: Option<RetentionDays>,
    ) -> Result<(), StoreError> {
        *self.2.lock().expect("the lock") = retention;
        Ok(())
    }

    async fn set_timezone(&self, _org: TenantId, timezone: &OrgTimezone) -> Result<(), StoreError> {
        *self.4.lock().expect("the lock") = timezone.clone();
        Ok(())
    }
}

struct Doors;

impl ApiKeys for Doors {
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
        _ttl: KeyTtl,
    ) -> Result<IssuedKey, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn revoke(&self, _org: TenantId, _id: KeyId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn grant_for(&self, secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok((secret.exposed() == READ_KEY)
            .then(|| ReadGrant::whole(TenantId::new(Uuid::from_u128(MINE)))))
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

impl RepoFor<domain::ApiKeys> for Fakes {
    type Repo = Doors;

    fn build(&self, (): &()) -> Self::Repo {
        Doors
    }
}

impl RepoFor<domain::Orgs> for Fakes {
    type Repo = Arc<Kept>;

    fn build(&self, (): &()) -> Self::Repo {
        Arc::clone(&self.kept)
    }
}

struct TestStack<P>(std::marker::PhantomData<P>);

impl<P: Photos + Send + Sync + 'static> Stack for TestStack<P> {
    type Repos = Fakes;
    type Clock = Inert;
    type Mailer = Inert;
    type Photos = P;
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

fn state<P: Photos + Send + Sync + 'static>(
    role: OrgRole,
    kept: &Arc<Kept>,
    photos: P,
) -> AppState<TestStack<P>> {
    let fakes = Fakes {
        role,
        kept: Arc::clone(kept),
    };
    let adapters = Adapters {
        clock: Inert::default(),
        mailer: Inert::default(),
        webhooks: Inert::default(),
        photos,
        meter: Inert::default(),
        span_store: None,
        issue_store: None,
        log_store: None,
        metric_store: None,
        users: Some(Bearers),
    };
    AppState::new(Arc::new(Ferristrace::new(fakes, adapters)))
}

fn org(seed: u128) -> String {
    Uuid::from_u128(seed).to_string()
}

fn session() -> String {
    format!("Bearer {TOKEN}")
}

fn asking(method: Method, seed: u128, body: Option<&str>) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("/api/orgs/{}", org(seed)))
        .header(AUTHORIZATION, session());
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    request
        .body(body.map_or_else(Body::empty, |json| Body::from(json.to_owned())))
        .expect("a request")
}

async fn served(role: OrgRole, kept: &Arc<Kept>, request: Request<Body>) -> (StatusCode, String) {
    let response = org_router(state(role, kept, NoPhotos))
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
    Arc::new(Kept(
        Mutex::new(name.to_owned()),
        Mutex::default(),
        Mutex::default(),
        None,
        Mutex::default(),
        Mutex::default(),
    ))
}

fn holding_under_cap(name: &str, cap: u32) -> Arc<Kept> {
    Arc::new(Kept(
        Mutex::new(name.to_owned()),
        Mutex::default(),
        Mutex::default(),
        RetentionDays::try_from(cap).ok(),
        Mutex::default(),
        Mutex::default(),
    ))
}

fn retaining(days: &serde_json::Value) -> String {
    serde_json::json!({ "days": days }).to_string()
}

fn asking_retention(seed: u128, body: &str) -> Request<Body> {
    Request::builder()
        .method(Method::PATCH)
        .uri(format!("/api/orgs/{}/retention", org(seed)))
        .header(AUTHORIZATION, session())
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .expect("a request")
}

fn a_png() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend_from_slice(b"and then whatever an encoder wrote");
    bytes
}

fn asking_logo(method: Method, seed: u128, body: Option<Vec<u8>>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(format!("/api/orgs/{}/logo", org(seed)))
        .header(AUTHORIZATION, session())
        .body(body.map_or_else(Body::empty, Body::from))
        .expect("a request")
}

async fn marked(
    role: OrgRole,
    kept: &Arc<Kept>,
    shelf: &Arc<Shelf>,
    request: Request<Body>,
) -> (StatusCode, Vec<u8>, Option<String>, Option<String>) {
    let response = org_router(state(role, kept, Arc::clone(shelf)))
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

    let (status, body) = served(OrgRole::Member, &kept, asking(Method::GET, MINE, None)).await;

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
async fn an_organisation_this_session_does_not_belong_to_is_not_found() {
    let kept = holding("Acme");

    let (status, _) = served(OrgRole::Owner, &kept, asking(Method::GET, THEIRS, None)).await;

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
        asking(Method::PATCH, MINE, Some(&renaming("Acme Payments"))),
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
        asking(Method::PATCH, MINE, Some(&renaming("Mine Now"))),
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
        asking(Method::PATCH, MINE, Some(&renaming("   "))),
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
        asking(Method::PATCH, MINE, Some(&renaming("  Acme  "))),
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
        asking_logo(Method::POST, MINE, Some(a_png())),
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
        asking_logo(Method::POST, MINE, Some(a_png())),
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
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let said = String::from_utf8(body).expect("text");
    assert!(said.contains("png"), "{said}");
    assert!(shelf.0.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn a_logo_is_dropped_and_the_card_comes_back_without_one() {
    let kept = holding("Acme");
    let shelf = Arc::new(Shelf::default());
    marked(
        OrgRole::Admin,
        &kept,
        &shelf,
        asking_logo(Method::POST, MINE, Some(a_png())),
    )
    .await;

    let (status, body, _, _) = marked(
        OrgRole::Admin,
        &kept,
        &shelf,
        asking_logo(Method::DELETE, MINE, None),
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
        asking_logo(Method::POST, MINE, Some(a_png())),
    )
    .await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("object store"), "{body}");
}

#[tokio::test]
async fn the_card_carries_the_retention_the_cap_and_the_deployment_default() {
    let kept = holding_under_cap("Acme", 90);

    let (status, body) = served(OrgRole::Member, &kept, asking(Method::GET, MINE, None)).await;

    assert_eq!(status, StatusCode::OK);
    let card: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert!(card["retention_days"].is_null());
    assert_eq!(card["retention_cap_days"], 90);
    assert_eq!(card["default_retention_days"], 30);
}

#[tokio::test]
async fn an_owner_sets_the_retention_and_is_answered_with_the_card() {
    let kept = holding("Acme");

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking_retention(MINE, &retaining(&serde_json::json!(7))),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let card: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(card["retention_days"], 7);
    assert!(card["retention_cap_days"].is_null());
}

#[tokio::test]
async fn null_days_returns_the_organisation_to_the_deployment_default() {
    let kept = holding("Acme");
    served(
        OrgRole::Owner,
        &kept,
        asking_retention(MINE, &retaining(&serde_json::json!(7))),
    )
    .await;

    let (status, body) = served(
        OrgRole::Owner,
        &kept,
        asking_retention(MINE, &retaining(&serde_json::Value::Null)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let card: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert!(card["retention_days"].is_null());
}

#[tokio::test]
async fn an_administrator_and_a_member_are_forbidden_to_set_the_retention() {
    for role in [OrgRole::Admin, OrgRole::Member] {
        let kept = holding("Acme");

        let (status, _) = served(
            role,
            &kept,
            asking_retention(MINE, &retaining(&serde_json::json!(7))),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(*kept.2.lock().expect("the lock"), None);
    }
}

#[tokio::test]
async fn a_stranger_setting_the_retention_of_another_organisation_finds_nothing() {
    let kept = holding("Acme");

    let (status, _) = served(
        OrgRole::Owner,
        &kept,
        asking_retention(THEIRS, &retaining(&serde_json::json!(7))),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_retention_out_of_range_or_above_the_plan_cap_is_unprocessable() {
    let kept = holding_under_cap("Acme", 30);

    for days in [
        serde_json::json!(0),
        serde_json::json!(3651),
        serde_json::json!(-1),
        serde_json::json!(31),
    ] {
        let (status, _) = served(
            OrgRole::Owner,
            &kept,
            asking_retention(MINE, &retaining(&days)),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{days}");
    }
    assert_eq!(*kept.2.lock().expect("the lock"), None);
}

#[tokio::test]
async fn a_body_with_no_days_does_not_quietly_reset_the_retention() {
    let kept = holding("Acme");

    let (status, _) = served(OrgRole::Owner, &kept, asking_retention(MINE, "{}")).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

fn zoning(seed: u128, body: &str) -> Request<Body> {
    Request::builder()
        .method(Method::PATCH)
        .uri(format!("/api/orgs/{}/timezone", org(seed)))
        .header(AUTHORIZATION, session())
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .expect("a request")
}

fn clock_of(seed: u128) -> Request<Body> {
    Request::builder()
        .uri(format!("/api/orgs/{}/clock", org(seed)))
        .header(AUTHORIZATION, session())
        .body(Body::empty())
        .expect("a request")
}

fn clock_by_key(seed: u128, secret: &str) -> Request<Body> {
    Request::builder()
        .uri(format!("/api/orgs/{}/clock", org(seed)))
        .header("authorization", format!("Bearer {secret}"))
        .body(Body::empty())
        .expect("a request")
}

#[tokio::test]
async fn the_card_carries_utc_until_an_administrator_picks_a_zone() {
    let kept = holding("Acme");

    let (_, body) = served(OrgRole::Member, &kept, asking(Method::GET, MINE, None)).await;
    let (status, changed) = served(
        OrgRole::Admin,
        &kept,
        zoning(MINE, r#"{"timezone":"Europe/Paris"}"#),
    )
    .await;
    let (_, after) = served(OrgRole::Member, &kept, asking(Method::GET, MINE, None)).await;

    assert_eq!(status, StatusCode::OK, "{changed}");
    let card = |text: &str| -> serde_json::Value { serde_json::from_str(text).expect("json") };
    assert_eq!(card(&body)["timezone"], "UTC");
    assert_eq!(card(&changed)["timezone"], "Europe/Paris");
    assert_eq!(card(&after)["timezone"], "Europe/Paris");
}

#[tokio::test]
async fn a_member_may_not_set_the_zone_and_a_stranger_finds_nothing() {
    let kept = holding("Acme");

    let (member, _) = served(
        OrgRole::Member,
        &kept,
        zoning(MINE, r#"{"timezone":"Europe/Paris"}"#),
    )
    .await;
    let (stranger, _) = served(
        OrgRole::Owner,
        &kept,
        zoning(THEIRS, r#"{"timezone":"Europe/Paris"}"#),
    )
    .await;

    assert_eq!(member, StatusCode::FORBIDDEN);
    assert_eq!(stranger, StatusCode::NOT_FOUND);
    assert_eq!(*kept.4.lock().expect("the lock"), OrgTimezone::utc());
}

#[tokio::test]
async fn a_zone_that_is_not_an_iana_name_is_unprocessable() {
    let kept = holding("Acme");

    for bad in [r#"{"timezone":"Mars/Olympus"}"#, r#"{"timezone":""}"#] {
        let (status, body) = served(OrgRole::Admin, &kept, zoning(MINE, bad)).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert!(body.contains("IANA"), "{body}");
    }
}

#[tokio::test]
async fn the_clock_answers_the_zone_to_a_session_and_to_a_read_key() {
    let kept = holding("Acme");
    served(
        OrgRole::Admin,
        &kept,
        zoning(MINE, r#"{"timezone":"Asia/Tokyo"}"#),
    )
    .await;

    let (session, by_session) = served(OrgRole::Member, &kept, clock_of(MINE)).await;
    let (key, by_key) = served(OrgRole::Member, &kept, clock_by_key(MINE, READ_KEY)).await;
    let (foreign, _) = served(OrgRole::Member, &kept, clock_by_key(THEIRS, READ_KEY)).await;
    let (unknown, _) = served(
        OrgRole::Member,
        &kept,
        clock_by_key(MINE, "ft_live_000.nope"),
    )
    .await;
    let (stranger, _) = served(OrgRole::Member, &kept, clock_of(THEIRS)).await;

    assert_eq!((session, key), (StatusCode::OK, StatusCode::OK));
    assert_eq!(by_session, r#"{"timezone":"Asia/Tokyo"}"#);
    assert_eq!(by_key, r#"{"timezone":"Asia/Tokyo"}"#);
    assert_eq!(foreign, StatusCode::NOT_FOUND);
    assert_eq!(unknown, StatusCode::UNAUTHORIZED);
    assert_eq!(stranger, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_logo_is_fetched_with_the_bearer() {
    let kept = holding("Acme");
    let shelf = Arc::new(Shelf::default());
    marked(
        OrgRole::Admin,
        &kept,
        &shelf,
        asking_logo(Method::POST, MINE, Some(a_png())),
    )
    .await;

    let (status, body, kind, caching) = marked(
        OrgRole::Member,
        &kept,
        &shelf,
        asking_logo(Method::GET, MINE, None),
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
        asking_logo(Method::GET, MINE, None),
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
        asking_logo(Method::GET, THEIRS, None),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
}

#[tokio::test]
async fn an_owner_disbands_it_and_is_answered_with_nothing_at_all() {
    let kept = holding("Acme");

    let (status, body) = served(OrgRole::Owner, &kept, asking(Method::DELETE, MINE, None)).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "{body}");
    let (after, _) = served(OrgRole::Owner, &kept, asking(Method::GET, MINE, None)).await;
    assert_eq!(after, StatusCode::NOT_FOUND, "the card is gone with it");
}

#[tokio::test]
async fn an_administrator_and_a_member_are_forbidden_to_disband_it() {
    for role in [OrgRole::Admin, OrgRole::Member] {
        let kept = holding("Acme");

        let (status, body) = served(role, &kept, asking(Method::DELETE, MINE, None)).await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{role:?}");
        assert!(body.contains("an owner's to do"), "{body}");
        let (after, _) = served(OrgRole::Owner, &kept, asking(Method::GET, MINE, None)).await;
        assert_eq!(
            after,
            StatusCode::OK,
            "the organisation outlived the refusal"
        );
    }
}

#[tokio::test]
async fn disbanding_an_organisation_this_session_does_not_belong_to_is_not_found() {
    let kept = holding("Acme");

    let (status, _) = served(OrgRole::Owner, &kept, asking(Method::DELETE, THEIRS, None)).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "telling a stranger an organisation exists is itself a leak"
    );
    let (after, _) = served(OrgRole::Owner, &kept, asking(Method::GET, MINE, None)).await;
    assert_eq!(after, StatusCode::OK);
}

#[tokio::test]
async fn disbanding_the_same_organisation_twice_is_not_found_the_second_time() {
    let kept = holding("Acme");
    let first = served(OrgRole::Owner, &kept, asking(Method::DELETE, MINE, None)).await;

    let (status, _) = served(OrgRole::Owner, &kept, asking(Method::DELETE, MINE, None)).await;

    assert_eq!(first.0, StatusCode::NO_CONTENT);
    assert_eq!(status, StatusCode::NOT_FOUND);
}
