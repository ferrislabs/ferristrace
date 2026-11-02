use std::collections::BTreeMap;
use std::sync::Mutex;
use uuid::Uuid;

use super::errors::{BadWindow, ExploreError};
use super::explore_service::Explore;
use super::ports::SpanReader;
use super::value_objects::{
    ServiceFilter, ServicePoint, ServiceSeries, ServiceSummary, TraceFilter, TraceOrder,
    TraceSummary, Window,
};
use crate::access::key_ring_tests::Ring;
use crate::access::{KeyScope, KeyScopes, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Agent, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::{Span, SpanId, SpanKind, SpanStatus, TenantId, TraceId};

const TOKEN: &str = "ft_ss_live";
const MINE: u128 = 1;
const THEIRS: u128 = 2;

fn an_email() -> Email {
    Email::try_from("person@example.com").expect("an address")
}

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn a_trace_id() -> TraceId {
    TraceId::try_from([7u8; 16].as_slice()).expect("sixteen bytes")
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
                email: an_email(),
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
}

impl Asked {
    fn projects(&self) -> Vec<Option<ProjectId>> {
        self.projects.lock().expect("the lock").clone()
    }
}

impl SpanReader for &Asked {
    async fn services(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        self.tenants.lock().expect("the lock").push(tenant);
        self.projects.lock().expect("the lock").push(filter.project);
        Ok(Vec::new())
    }

    async fn operations(
        &self,
        tenant: TenantId,
        service: &str,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        self.tenants.lock().expect("the lock").push(tenant);
        self.projects.lock().expect("the lock").push(filter.project);
        Ok(vec![ServiceSummary {
            name: format!("{service} op").into(),
            requests: 4,
            errors: 1,
            p50_nanos: 1,
            p95_nanos: 2,
            p99_nanos: 3,
            last_seen_unix_nanos: 9,
        }])
    }

    async fn series(
        &self,
        tenant: TenantId,
        _service: Option<&str>,
        filter: &ServiceFilter,
    ) -> Result<ServiceSeries, StoreError> {
        self.tenants.lock().expect("the lock").push(tenant);
        self.projects.lock().expect("the lock").push(filter.project);
        let start = filter.window.from_unix_nanos() + 2 * filter.step_nanos();
        Ok(ServiceSeries {
            step_nanos: filter.step_nanos(),
            requests: 5,
            errors: 2,
            p95_nanos: Some(70),
            points: vec![ServicePoint {
                start_unix_nanos: start,
                requests: 5,
                errors: 2,
                p95_nanos: Some(70),
            }],
        })
    }

    async fn traces(
        &self,
        tenant: TenantId,
        filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        self.tenants.lock().expect("the lock").push(tenant);
        self.projects.lock().expect("the lock").push(filter.project);
        Ok(vec![TraceSummary {
            trace_id: a_trace_id(),
            root_service: "checkout".into(),
            root_name: "pay".into(),
            started_unix_nanos: 1,
            duration_nanos: 2,
            spans: 3,
            errors: 0,
        }])
    }

    async fn trace(
        &self,
        tenant: TenantId,
        id: TraceId,
        project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        self.tenants.lock().expect("the lock").push(tenant);
        self.projects.lock().expect("the lock").push(project);
        if project.is_some_and(|asked| asked != a_project(OURS)) {
            return Ok(Vec::new());
        }
        Ok(vec![Span {
            trace_id: id,
            span_id: SpanId::try_from([1u8; 8].as_slice()).expect("eight bytes"),
            parent: None,
            name: "work".into(),
            service_name: "checkout".into(),
            kind: SpanKind::Server,
            status: SpanStatus::Ok,
            start_unix_nanos: 1,
            end_unix_nanos: 2,
            attributes: BTreeMap::new(),
            events: Vec::new(),
        }])
    }
}

struct Unreachable;

impl SpanReader for Unreachable {
    async fn services(
        &self,
        _tenant: TenantId,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn operations(
        &self,
        _tenant: TenantId,
        _service: &str,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn series(
        &self,
        _tenant: TenantId,
        _service: Option<&str>,
        _filter: &ServiceFilter,
    ) -> Result<ServiceSeries, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        _filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn trace(
        &self,
        _tenant: TenantId,
        _id: TraceId,
        _project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        Err(StoreError::Unreachable)
    }
}

fn elsewhere(window: Window) -> ServiceFilter {
    let mut filter = ServiceFilter::over(window);
    filter.project = Some(a_project(OTHER));
    filter
}

fn an_hour() -> ServiceFilter {
    ServiceFilter::over(Window::new(0, 3_600_000_000_000).expect("an hour"))
}

fn a_filter() -> TraceFilter {
    TraceFilter::over(Window::new(0, 1000).expect("a window"))
}

fn exploring<R: SpanReader>(reader: R) -> Explore<OneSession, MemberOfMine, NoKeys, R> {
    Explore::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine), &*NO_KEYS),
        reader,
    )
}

fn keyed<R: SpanReader>(keys: &Ring, reader: R) -> Explore<OneSession, MemberOfMine, &Ring, R> {
    Explore::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine), keys),
        reader,
    )
}

type NoKeys = &'static Ring;

static NO_KEYS: std::sync::LazyLock<Ring> = std::sync::LazyLock::new(Ring::default);

fn session(plaintext: &str) -> Reader {
    Reader::Session(SessionToken::from_plaintext(plaintext))
}

const OURS: u128 = 30;
const OTHER: u128 = 40;

fn a_project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn reading() -> KeyScopes {
    KeyScopes::new(&[KeyScope::Read]).expect("scopes")
}

#[tokio::test]
async fn a_member_reads_the_traces_of_the_organisation_it_belongs_to() {
    let asked = Asked::default();
    let explore = exploring(&asked);

    let found = explore
        .traces(&session(TOKEN), an_org(MINE), &a_filter())
        .await
        .expect("a member may read");

    assert_eq!(found.len(), 1);
    assert_eq!(
        *asked.tenants.lock().expect("the lock"),
        vec![an_org(MINE)],
        "the store is asked for the organisation that was authorised"
    );
}

#[tokio::test]
async fn an_organisation_this_session_does_not_belong_to_is_refused() {
    let asked = Asked::default();
    let explore = exploring(&asked);

    let refused = explore
        .traces(&session(TOKEN), an_org(THEIRS), &a_filter())
        .await;

    assert_eq!(refused, Err(ExploreError::NotYours));
    assert!(
        asked.tenants.lock().expect("the lock").is_empty(),
        "the span store must not be asked at all"
    );
}

#[tokio::test]
async fn a_single_trace_is_refused_to_a_stranger_too() {
    let asked = Asked::default();
    let explore = exploring(&asked);

    let refused = explore
        .trace(&session(TOKEN), an_org(THEIRS), a_trace_id())
        .await;

    assert_eq!(refused, Err(ExploreError::NotYours));
    assert!(asked.tenants.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn a_token_no_session_carries_is_refused_before_anything_is_read() {
    let asked = Asked::default();
    let explore = exploring(&asked);

    let refused = explore
        .traces(&session("ft_ss_nope"), an_org(MINE), &a_filter())
        .await;

    assert_eq!(refused, Err(ExploreError::NoSession));
    assert!(asked.tenants.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn an_unreachable_span_store_is_not_reported_as_a_refusal() {
    let explore = exploring(Unreachable);

    let failed = explore
        .traces(&session(TOKEN), an_org(MINE), &a_filter())
        .await;

    assert_eq!(failed, Err(ExploreError::StoreUnreachable));
}

#[tokio::test]
async fn the_services_of_another_organisation_are_refused_before_the_store_is_asked() {
    let asked = Asked::default();
    let explore = exploring(&asked);
    let window = Window::new(0, 1000).expect("a window");

    let refused = explore
        .services(
            &session(TOKEN),
            an_org(THEIRS),
            &ServiceFilter::over(window),
        )
        .await;

    assert_eq!(refused, Err(ExploreError::NotYours));
    assert!(asked.tenants.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn a_member_reads_the_services_of_its_own_organisation() {
    let asked = Asked::default();
    let explore = exploring(&asked);
    let window = Window::new(0, 1000).expect("a window");

    explore
        .services(&session(TOKEN), an_org(MINE), &ServiceFilter::over(window))
        .await
        .expect("a member may read");

    assert_eq!(*asked.tenants.lock().expect("the lock"), vec![an_org(MINE)]);
}

#[test]
fn a_window_must_end_after_it_starts() {
    assert_eq!(Window::new(10, 10), Err(BadWindow::Backwards));
    assert_eq!(Window::new(10, 9), Err(BadWindow::Backwards));
    assert!(Window::new(9, 10).is_ok());
}

#[test]
fn a_limit_is_held_between_one_and_five_hundred() {
    assert_eq!(a_filter().limit(), 50, "the default asks for a screenful");
    assert_eq!(a_filter().at_most(0).limit(), 1);
    assert_eq!(a_filter().at_most(10_000).limit(), 500);
    assert_eq!(a_filter().at_most(120).limit(), 120);
}

#[test]
fn a_filter_asks_for_everything_until_it_is_narrowed() {
    let filter = a_filter();

    assert_eq!(filter.service, None);
    assert!(!filter.only_errors);
    assert_eq!(filter.at_least_nanos, None);
    assert_eq!(filter.order, TraceOrder::Started);
}

#[tokio::test]
async fn a_project_bound_read_key_reads_only_its_project_when_none_is_named() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let asked = Asked::default();
    let explore = keyed(&keys, &asked);
    let window = Window::new(0, 1000).expect("a window");

    explore
        .traces(&Reader::Key(secret.clone()), an_org(MINE), &a_filter())
        .await
        .expect("a read key may read its project");
    explore
        .services(
            &Reader::Key(secret),
            an_org(MINE),
            &ServiceFilter::over(window),
        )
        .await
        .expect("and its services");

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
    let explore = keyed(&keys, &asked);
    let mut filter = a_filter();
    filter.project = Some(a_project(OTHER));
    let window = Window::new(0, 1000).expect("a window");

    let traces = explore
        .traces(&Reader::Key(secret.clone()), an_org(MINE), &filter)
        .await;
    let services = explore
        .services(&Reader::Key(secret), an_org(MINE), &elsewhere(window))
        .await;

    assert_eq!(traces, Err(ExploreError::NotYours));
    assert_eq!(services, Err(ExploreError::NotYours));
    assert!(asked.projects().is_empty(), "the store is never asked");
}

#[tokio::test]
async fn a_project_bound_read_key_opens_a_trace_of_its_own_project() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let asked = Asked::default();

    let spans = keyed(&keys, &asked)
        .trace(&Reader::Key(secret), an_org(MINE), a_trace_id())
        .await
        .expect("a trace of its own project");

    assert_eq!(spans.len(), 1);
    assert_eq!(asked.projects(), vec![Some(a_project(OURS))]);
}

#[tokio::test]
async fn a_trace_only_another_project_holds_reads_as_empty_for_a_project_bound_key() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OTHER)), reading());
    let asked = Asked::default();

    let spans = keyed(&keys, &asked)
        .trace(&Reader::Key(secret), an_org(MINE), a_trace_id())
        .await
        .expect("an unknown trace is no error");

    assert!(spans.is_empty(), "no span of another project, ever");
    assert_eq!(asked.projects(), vec![Some(a_project(OTHER))]);
}

#[tokio::test]
async fn a_session_reads_a_trace_without_a_project_restriction() {
    let asked = Asked::default();

    exploring(&asked)
        .trace(&session(TOKEN), an_org(MINE), a_trace_id())
        .await
        .expect("a member may read");

    assert_eq!(asked.projects(), vec![None]);
}

#[tokio::test]
async fn an_organisation_wide_read_key_reads_a_trace_and_any_project() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, reading());
    let asked = Asked::default();
    let explore = keyed(&keys, &asked);
    let mut filter = a_filter();
    filter.project = Some(a_project(OTHER));

    let spans = explore
        .trace(&Reader::Key(secret.clone()), an_org(MINE), a_trace_id())
        .await
        .expect("a whole organisation key may read a trace");
    explore
        .traces(&Reader::Key(secret), an_org(MINE), &filter)
        .await
        .expect("and any project of it");

    assert_eq!(spans.len(), 1);
    assert_eq!(asked.projects(), vec![None, Some(a_project(OTHER))]);
}

#[tokio::test]
async fn a_read_key_reads_nothing_of_another_organisation() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, reading());
    let asked = Asked::default();

    let refused = keyed(&keys, &asked)
        .traces(&Reader::Key(secret), an_org(THEIRS), &a_filter())
        .await;

    assert_eq!(refused, Err(ExploreError::NotYours));
    assert!(asked.tenants.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn an_ingest_key_reads_nothing_and_is_refused_as_unauthenticated() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, KeyScopes::ingest());
    let asked = Asked::default();

    let refused = keyed(&keys, &asked)
        .traces(&Reader::Key(secret), an_org(MINE), &a_filter())
        .await;

    assert_eq!(refused, Err(ExploreError::NoSession));
    assert!(asked.tenants.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn a_member_reads_the_operations_of_a_service_of_its_organisation() {
    let asked = Asked::default();

    let found = exploring(&asked)
        .operations(&session(TOKEN), an_org(MINE), "checkout", &an_hour())
        .await
        .expect("a member may read");

    assert_eq!(&*found[0].name, "checkout op");
    assert_eq!(*asked.tenants.lock().expect("the lock"), vec![an_org(MINE)]);
}

#[tokio::test]
async fn the_operations_and_series_of_another_organisation_are_refused_before_the_store_is_asked() {
    let asked = Asked::default();
    let explore = exploring(&asked);

    let operations = explore
        .operations(&session(TOKEN), an_org(THEIRS), "checkout", &an_hour())
        .await;
    let series = explore
        .series(
            &session(TOKEN),
            an_org(THEIRS),
            Some("checkout"),
            &an_hour(),
        )
        .await;

    assert_eq!(operations, Err(ExploreError::NotYours));
    assert_eq!(series, Err(ExploreError::NotYours));
    assert!(asked.tenants.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn a_project_bound_read_key_reads_operations_and_series_of_its_project_only() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let asked = Asked::default();
    let explore = keyed(&keys, &asked);
    let window = Window::new(0, 3_600_000_000_000).expect("an hour");

    explore
        .operations(
            &Reader::Key(secret.clone()),
            an_org(MINE),
            "checkout",
            &an_hour(),
        )
        .await
        .expect("its operations");
    explore
        .series(&Reader::Key(secret.clone()), an_org(MINE), None, &an_hour())
        .await
        .expect("its series");
    let operations = explore
        .operations(
            &Reader::Key(secret.clone()),
            an_org(MINE),
            "checkout",
            &elsewhere(window),
        )
        .await;
    let series = explore
        .series(
            &Reader::Key(secret),
            an_org(MINE),
            Some("checkout"),
            &elsewhere(window),
        )
        .await;

    assert_eq!(operations, Err(ExploreError::NotYours));
    assert_eq!(series, Err(ExploreError::NotYours));
    assert_eq!(
        asked.projects(),
        vec![Some(a_project(OURS)), Some(a_project(OURS))]
    );
}

#[tokio::test]
async fn a_series_has_a_point_for_every_step_even_where_nothing_was_served() {
    let asked = Asked::default();
    let filter = an_hour();

    let series = exploring(&asked)
        .series(&session(TOKEN), an_org(MINE), Some("checkout"), &filter)
        .await
        .expect("a member may read");

    assert_eq!(series.step_nanos, 60_000_000_000);
    assert_eq!(series.points.len(), 60);
    assert_eq!(series.points[0].start_unix_nanos, 0);
    assert_eq!(series.points[59].start_unix_nanos, 59 * 60_000_000_000);
    assert_eq!(
        series.points[1],
        ServicePoint {
            start_unix_nanos: 60_000_000_000,
            requests: 0,
            errors: 0,
            p95_nanos: None,
        }
    );
    assert_eq!(series.points[2].requests, 5);
    assert_eq!(series.points[2].p95_nanos, Some(70));
    assert_eq!((series.requests, series.errors), (5, 2));
}

#[tokio::test]
async fn an_unreachable_store_fails_operations_and_series_as_unreachable() {
    let explore = exploring(Unreachable);

    assert_eq!(
        explore
            .operations(&session(TOKEN), an_org(MINE), "checkout", &an_hour())
            .await,
        Err(ExploreError::StoreUnreachable)
    );
    assert_eq!(
        explore
            .series(&session(TOKEN), an_org(MINE), None, &an_hour())
            .await,
        Err(ExploreError::StoreUnreachable)
    );
}

#[test]
fn a_service_filter_steps_a_sixtieth_of_its_window_and_never_under_a_second() {
    let minute = ServiceFilter::over(Window::new(0, 60_000_000_000).expect("a minute"));
    let short = ServiceFilter::over(Window::new(0, 1_000_000).expect("a millisecond"));

    assert_eq!(an_hour().step_nanos(), 60_000_000_000);
    assert_eq!(minute.step_nanos(), 1_000_000_000);
    assert_eq!(short.step_nanos(), 1_000_000_000);
    assert_eq!(short.slot_starts().count(), 1);
    assert_eq!(an_hour().environment, None);
}
