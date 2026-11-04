use std::sync::Mutex;
use uuid::Uuid;

use super::entities::MetricDescriptor;
use super::errors::{BadMetricQuery, MetricError};
use super::metric_search::MetricSearch;
use super::ports::MetricReader;
use super::value_objects::{Aggregation, MetricKind, MetricQuery, SeriesSet};
use crate::access::key_ring_tests::Ring;
use crate::access::{KeyScope, KeyScopes, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::clock::UnixSeconds;
use crate::explore::{ExploreError, Window};
use crate::identity::{AccountId, Email, Session, SessionError, SessionStore};
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
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
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

type Charted = (TenantId, MetricKind, Aggregation, Option<ProjectId>);
type Listed = (TenantId, Option<Box<str>>, Option<ProjectId>);

#[derive(Default)]
struct Asked {
    listed: Mutex<Vec<Listed>>,
    charted: Mutex<Vec<Charted>>,
    placements: Mutex<Vec<Placement>>,
}

impl Asked {
    fn listed(&self) -> Vec<Listed> {
        self.listed.lock().expect("the lock").clone()
    }

    fn charted(&self) -> Vec<Charted> {
        self.charted.lock().expect("the lock").clone()
    }
}

fn described(name: &str, kind: MetricKind) -> MetricDescriptor {
    MetricDescriptor {
        name: name.into(),
        kind,
        unit: "1".into(),
        description: String::new().into(),
        last_seen_unix_nanos: 5,
    }
}

impl MetricReader for &Asked {
    async fn catalogue(
        &self,
        org: TenantId,
        search: Option<&str>,
        project: Option<ProjectId>,
    ) -> Result<Vec<MetricDescriptor>, StoreError> {
        self.listed
            .lock()
            .expect("the lock")
            .push((org, search.map(Into::into), project));
        Ok(vec![
            described("http.requests", MetricKind::Sum),
            described("http.requests.duration", MetricKind::Histogram),
            described("queue.depth", MetricKind::Gauge),
        ]
        .into_iter()
        .filter(|metric| search.is_none_or(|text| metric.name.contains(text)))
        .collect())
    }

    async fn series(
        &self,
        org: TenantId,
        kind: MetricKind,
        aggregation: Aggregation,
        query: &MetricQuery,
    ) -> Result<SeriesSet, StoreError> {
        self.charted
            .lock()
            .expect("the lock")
            .push((org, kind, aggregation, query.project));
        self.placements
            .lock()
            .expect("the lock")
            .push(query.placement.clone());
        Ok(SeriesSet {
            step_nanos: query.step_nanos(),
            series: Vec::new(),
            exemplars: Vec::new(),
        })
    }
}

struct Unreachable;

impl MetricReader for Unreachable {
    async fn catalogue(
        &self,
        _org: TenantId,
        _search: Option<&str>,
        _project: Option<ProjectId>,
    ) -> Result<Vec<MetricDescriptor>, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn series(
        &self,
        _org: TenantId,
        _kind: MetricKind,
        _aggregation: Aggregation,
        _query: &MetricQuery,
    ) -> Result<SeriesSet, StoreError> {
        Err(StoreError::Unreachable)
    }
}

static NO_KEYS: std::sync::LazyLock<Ring> = std::sync::LazyLock::new(Ring::default);

fn searching<R: MetricReader>(
    keys: &Ring,
    reader: R,
) -> MetricSearch<OneSession, MemberOfMine, &Ring, R> {
    MetricSearch::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine), keys),
        reader,
    )
}

fn session() -> Reader {
    Reader::Session(SessionToken::for_subject(TOKEN))
}

fn a_query(name: &str) -> MetricQuery {
    MetricQuery::over(Window::new(0, 60_000_000_000).expect("a window"), name)
}

fn reading() -> KeyScopes {
    KeyScopes::new(&[KeyScope::Read]).expect("scopes")
}

#[tokio::test]
async fn a_member_lists_the_metrics_of_its_own_organisation() {
    let asked = Asked::default();

    let listed = searching(&NO_KEYS, &asked)
        .catalogue(&session(), an_org(MINE), Some("http"), None)
        .await
        .expect("a member may read");

    assert_eq!(listed.len(), 2);
    assert_eq!(
        asked.listed(),
        vec![(an_org(MINE), Some(Box::from("http")), None)]
    );
}

#[tokio::test]
async fn a_series_is_charted_with_the_kind_the_catalogue_holds() {
    let asked = Asked::default();

    let charted = searching(&NO_KEYS, &asked)
        .series(&session(), an_org(MINE), &a_query("http.requests"))
        .await
        .expect("a member may chart");

    assert_eq!(charted.step_nanos, 1_000_000_000);
    assert_eq!(
        asked.charted(),
        vec![(an_org(MINE), MetricKind::Sum, Aggregation::Rate, None)]
    );
}

#[tokio::test]
async fn a_metric_the_catalogue_does_not_hold_is_unknown() {
    let asked = Asked::default();

    let refused = searching(&NO_KEYS, &asked)
        .series(&session(), an_org(MINE), &a_query("http"))
        .await;

    assert_eq!(refused, Err(MetricError::Unknown));
    assert!(asked.charted().is_empty());
}

#[tokio::test]
async fn an_aggregation_that_does_not_fit_the_kind_is_refused_before_the_store_is_asked() {
    let asked = Asked::default();
    let mut query = a_query("queue.depth");
    query.aggregation = Some(Aggregation::P95);

    let refused = searching(&NO_KEYS, &asked)
        .series(&session(), an_org(MINE), &query)
        .await;

    assert_eq!(
        refused,
        Err(MetricError::Query(BadMetricQuery::Inapplicable {
            aggregation: Aggregation::P95,
            kind: MetricKind::Gauge,
        }))
    );
    assert!(asked.charted().is_empty());
}

#[tokio::test]
async fn the_metrics_of_an_organisation_this_session_does_not_belong_to_are_refused() {
    let asked = Asked::default();
    let search = searching(&NO_KEYS, &asked);

    let listed = search
        .catalogue(&session(), an_org(THEIRS), None, None)
        .await;
    let charted = search
        .series(&session(), an_org(THEIRS), &a_query("http.requests"))
        .await;

    assert_eq!(listed, Err(ExploreError::NotYours));
    assert_eq!(charted, Err(MetricError::NotYours));
    assert!(asked.listed().is_empty(), "the metric store is never asked");
}

#[tokio::test]
async fn a_project_bound_read_key_reads_only_its_project() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let asked = Asked::default();
    let search = searching(&keys, &asked);

    search
        .catalogue(&Reader::Key(secret.clone()), an_org(MINE), None, None)
        .await
        .expect("a read key lists its project");
    search
        .series(
            &Reader::Key(secret),
            an_org(MINE),
            &a_query("http.requests"),
        )
        .await
        .expect("and charts its project");

    assert_eq!(
        asked
            .listed()
            .into_iter()
            .map(|(_, _, project)| project)
            .collect::<Vec<_>>(),
        vec![Some(a_project(OURS)), Some(a_project(OURS))]
    );
    assert_eq!(asked.charted()[0].3, Some(a_project(OURS)));
}

#[tokio::test]
async fn a_project_bound_read_key_naming_another_project_is_refused_like_a_stranger() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let asked = Asked::default();
    let search = searching(&keys, &asked);
    let mut query = a_query("http.requests");
    query.project = Some(a_project(OTHER));

    let listed = search
        .catalogue(
            &Reader::Key(secret.clone()),
            an_org(MINE),
            None,
            Some(a_project(OTHER)),
        )
        .await;
    let charted = search
        .series(&Reader::Key(secret), an_org(MINE), &query)
        .await;

    assert_eq!(listed, Err(ExploreError::NotYours));
    assert_eq!(charted, Err(MetricError::NotYours));
    assert!(asked.listed().is_empty());
}

#[tokio::test]
async fn an_organisation_wide_read_key_keeps_the_project_it_names() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, reading());
    let asked = Asked::default();
    let mut query = a_query("http.requests");
    query.project = Some(a_project(OTHER));

    searching(&keys, &asked)
        .series(&Reader::Key(secret), an_org(MINE), &query)
        .await
        .expect("an organisation key reads any project");

    assert_eq!(asked.charted()[0].3, Some(a_project(OTHER)));
}

#[tokio::test]
async fn an_ingest_key_reads_no_metric() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, KeyScopes::ingest());
    let asked = Asked::default();

    let refused = searching(&keys, &asked)
        .catalogue(&Reader::Key(secret), an_org(MINE), None, None)
        .await;

    assert_eq!(refused, Err(ExploreError::NoSession));
    assert!(asked.listed().is_empty());
}

#[tokio::test]
async fn an_unreachable_metric_store_is_not_reported_as_a_refusal() {
    let search = searching(&NO_KEYS, Unreachable);

    let listed = search.catalogue(&session(), an_org(MINE), None, None).await;
    let charted = search
        .series(&session(), an_org(MINE), &a_query("http.requests"))
        .await;

    assert_eq!(listed, Err(ExploreError::StoreUnreachable));
    assert_eq!(charted, Err(MetricError::StoreUnreachable));
}

#[tokio::test]
async fn a_placement_reaches_the_store_when_a_key_narrows_the_project() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let asked = Asked::default();
    let mut query = a_query("queue.depth");
    query.placement = Placement {
        namespace: Some(PlacementName::try_from("shop").expect("a namespace")),
        ..Placement::default()
    };

    searching(&keys, &asked)
        .series(&Reader::Key(secret), an_org(MINE), &query)
        .await
        .expect("its series");

    assert_eq!(
        *asked.placements.lock().expect("the lock"),
        vec![query.placement.clone()]
    );
    assert_eq!(asked.charted()[0].3, Some(a_project(OURS)));
    assert!(a_query("queue.depth").placement.is_empty());
}
