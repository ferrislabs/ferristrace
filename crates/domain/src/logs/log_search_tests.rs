use std::sync::Mutex;
use uuid::Uuid;

use super::log_search::LogSearch;
use super::ports::LogReader;
use super::value_objects::{Buckets, LogBucket, LogPage, LogQuery};
use crate::access::key_ring_tests::Ring;
use crate::access::{KeyScope, KeyScopes, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::clock::UnixSeconds;
use crate::explore::{ExploreError, Window};
use crate::identity::{AccountId, Agent, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
use crate::placement::{Placement, PlacementName};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::TenantId;

const TOKEN: &str = "ft_ss_live";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const OURS: u128 = 30;
const OTHER: u128 = 40;

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn a_project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
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
                email: Email::try_from("person@example.com").expect("an address"),
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

struct MemberOfMine;

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: an_org(MINE),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

#[derive(Default)]
struct Asked {
    tenants: Mutex<Vec<TenantId>>,
    projects: Mutex<Vec<Option<ProjectId>>>,
    placements: Mutex<Vec<Placement>>,
}

impl Asked {
    fn tenants(&self) -> Vec<TenantId> {
        self.tenants.lock().expect("the lock").clone()
    }

    fn projects(&self) -> Vec<Option<ProjectId>> {
        self.projects.lock().expect("the lock").clone()
    }

    fn note(&self, org: TenantId, query: &LogQuery) {
        self.tenants.lock().expect("the lock").push(org);
        self.projects.lock().expect("the lock").push(query.project);
        self.placements
            .lock()
            .expect("the lock")
            .push(query.placement.clone());
    }
}

impl LogReader for &Asked {
    async fn search(&self, org: TenantId, query: &LogQuery) -> Result<LogPage, StoreError> {
        self.note(org, query);
        Ok(LogPage::default())
    }

    async fn histogram(
        &self,
        org: TenantId,
        query: &LogQuery,
        buckets: Buckets,
    ) -> Result<Vec<LogBucket>, StoreError> {
        self.note(org, query);
        Ok(buckets.laid_over(query.window))
    }
}

struct Unreachable;

impl LogReader for Unreachable {
    async fn search(&self, _org: TenantId, _query: &LogQuery) -> Result<LogPage, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn histogram(
        &self,
        _org: TenantId,
        _query: &LogQuery,
        _buckets: Buckets,
    ) -> Result<Vec<LogBucket>, StoreError> {
        Err(StoreError::Unreachable)
    }
}

static NO_KEYS: std::sync::LazyLock<Ring> = std::sync::LazyLock::new(Ring::default);

fn searching<R: LogReader>(
    keys: &Ring,
    reader: R,
) -> LogSearch<OneSession, MemberOfMine, &Ring, R> {
    LogSearch::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine), keys),
        reader,
    )
}

fn session() -> Reader {
    Reader::Session(SessionToken::from_plaintext(TOKEN))
}

fn a_query() -> LogQuery {
    LogQuery::over(Window::new(0, 1000).expect("a window"))
}

fn reading() -> KeyScopes {
    KeyScopes::new(&[KeyScope::Read]).expect("scopes")
}

#[tokio::test]
async fn a_member_searches_the_logs_of_its_own_organisation() {
    let asked = Asked::default();

    searching(&NO_KEYS, &asked)
        .search(&session(), an_org(MINE), &a_query())
        .await
        .expect("a member may read");

    assert_eq!(asked.tenants(), vec![an_org(MINE)]);
    assert_eq!(asked.projects(), vec![None]);
}

#[tokio::test]
async fn the_logs_of_an_organisation_this_session_does_not_belong_to_are_refused() {
    let asked = Asked::default();
    let search = searching(&NO_KEYS, &asked);

    let page = search.search(&session(), an_org(THEIRS), &a_query()).await;
    let histogram = search
        .histogram(&session(), an_org(THEIRS), &a_query(), Buckets::default())
        .await;

    assert_eq!(page, Err(ExploreError::NotYours));
    assert_eq!(histogram, Err(ExploreError::NotYours));
    assert!(asked.tenants().is_empty(), "the log store is never asked");
}

#[tokio::test]
async fn a_token_no_session_carries_reads_no_log() {
    let asked = Asked::default();

    let refused = searching(&NO_KEYS, &asked)
        .search(
            &Reader::Session(SessionToken::from_plaintext("ft_ss_nope")),
            an_org(MINE),
            &a_query(),
        )
        .await;

    assert_eq!(refused, Err(ExploreError::NoSession));
    assert!(asked.tenants().is_empty());
}

#[tokio::test]
async fn a_project_bound_read_key_searches_only_its_project() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let asked = Asked::default();
    let search = searching(&keys, &asked);

    search
        .search(&Reader::Key(secret.clone()), an_org(MINE), &a_query())
        .await
        .expect("a read key reads its project");
    search
        .histogram(
            &Reader::Key(secret),
            an_org(MINE),
            &a_query(),
            Buckets::default(),
        )
        .await
        .expect("and counts its project");

    assert_eq!(
        asked.projects(),
        vec![Some(a_project(OURS)), Some(a_project(OURS))]
    );
}

#[tokio::test]
async fn a_project_bound_read_key_naming_another_project_is_refused_like_a_stranger() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let asked = Asked::default();
    let search = searching(&keys, &asked);
    let mut query = a_query();
    query.project = Some(a_project(OTHER));

    let page = search
        .search(&Reader::Key(secret.clone()), an_org(MINE), &query)
        .await;
    let histogram = search
        .histogram(
            &Reader::Key(secret),
            an_org(MINE),
            &query,
            Buckets::default(),
        )
        .await;

    assert_eq!(page, Err(ExploreError::NotYours));
    assert_eq!(histogram, Err(ExploreError::NotYours));
    assert!(asked.projects().is_empty());
}

#[tokio::test]
async fn an_organisation_wide_read_key_keeps_the_project_it_names() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, reading());
    let asked = Asked::default();
    let mut query = a_query();
    query.project = Some(a_project(OTHER));

    searching(&keys, &asked)
        .search(&Reader::Key(secret), an_org(MINE), &query)
        .await
        .expect("an organisation key reads any project");

    assert_eq!(asked.projects(), vec![Some(a_project(OTHER))]);
}

#[tokio::test]
async fn an_ingest_key_reads_no_log() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, KeyScopes::ingest());
    let asked = Asked::default();

    let refused = searching(&keys, &asked)
        .search(&Reader::Key(secret), an_org(MINE), &a_query())
        .await;

    assert_eq!(refused, Err(ExploreError::NoSession));
    assert!(asked.tenants().is_empty());
}

#[tokio::test]
async fn an_unreachable_log_store_is_not_reported_as_a_refusal() {
    let refused = searching(&NO_KEYS, Unreachable)
        .search(&session(), an_org(MINE), &a_query())
        .await;

    assert_eq!(refused, Err(ExploreError::StoreUnreachable));
}

#[tokio::test]
async fn a_placement_reaches_the_store_when_a_key_narrows_the_project() {
    let keys = Ring::default();
    let secret = keys.holding(
        an_org(MINE),
        Some(a_project(OURS)),
        KeyScopes::new(&[KeyScope::Read]).expect("scopes"),
    );
    let asked = Asked::default();
    let mut query = LogQuery::over(Window::new(0, 1000).expect("a window"));
    query.placement = Placement {
        node: Some(PlacementName::try_from("node-a").expect("a node")),
        ..Placement::default()
    };
    let search = searching(&keys, &asked);

    search
        .search(&Reader::Key(secret.clone()), an_org(MINE), &query)
        .await
        .expect("its logs");
    search
        .histogram(
            &Reader::Key(secret),
            an_org(MINE),
            &query,
            Buckets::default(),
        )
        .await
        .expect("its histogram");

    assert_eq!(
        *asked.placements.lock().expect("the lock"),
        vec![query.placement.clone(), query.placement.clone()]
    );
    assert_eq!(asked.projects(), vec![Some(a_project(OURS)); 2]);
    assert!(LogQuery::over(query.window).placement.is_empty());
}
