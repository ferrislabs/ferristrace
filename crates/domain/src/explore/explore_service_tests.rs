use std::collections::BTreeMap;
use std::sync::Mutex;
use uuid::Uuid;

use super::errors::{BadWindow, ExploreError, ReadError};
use super::explore_service::Explore;
use super::ports::SpanReader;
use super::value_objects::{ServiceSummary, TraceFilter, TraceOrder, TraceSummary, Window};
use crate::catalogue::ProjectId;
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Agent, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::telemetry::{Span, SpanId, SpanKind, SpanStatus, TenantId, TraceId};

const TOKEN: &str = "ct_ss_live";
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
}

impl SpanReader for &Asked {
    async fn services(
        &self,
        tenant: TenantId,
        _window: Window,
        _project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, ReadError> {
        self.tenants.lock().expect("the lock").push(tenant);
        Ok(Vec::new())
    }

    async fn traces(
        &self,
        tenant: TenantId,
        _filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, ReadError> {
        self.tenants.lock().expect("the lock").push(tenant);
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

    async fn trace(&self, tenant: TenantId, id: TraceId) -> Result<Vec<Span>, ReadError> {
        self.tenants.lock().expect("the lock").push(tenant);
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
        _window: Window,
        _project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, ReadError> {
        Err(ReadError::StoreUnreachable)
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        _filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, ReadError> {
        Err(ReadError::StoreUnreachable)
    }

    async fn trace(&self, _tenant: TenantId, _id: TraceId) -> Result<Vec<Span>, ReadError> {
        Err(ReadError::StoreUnreachable)
    }
}

fn a_filter() -> TraceFilter {
    TraceFilter::over(Window::new(0, 1000).expect("a window"))
}

fn exploring<R: SpanReader>(reader: R) -> Explore<OneSession, MemberOfMine, R> {
    Explore::new(Whoami::new(OneSession, MemberOfMine), reader)
}

#[tokio::test]
async fn a_member_reads_the_traces_of_the_organisation_it_belongs_to() {
    let asked = Asked::default();
    let explore = exploring(&asked);

    let found = explore
        .traces(
            &SessionToken::from_plaintext(TOKEN),
            an_org(MINE),
            &a_filter(),
        )
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
        .traces(
            &SessionToken::from_plaintext(TOKEN),
            an_org(THEIRS),
            &a_filter(),
        )
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
        .trace(
            &SessionToken::from_plaintext(TOKEN),
            an_org(THEIRS),
            a_trace_id(),
        )
        .await;

    assert_eq!(refused, Err(ExploreError::NotYours));
    assert!(asked.tenants.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn a_token_no_session_carries_is_refused_before_anything_is_read() {
    let asked = Asked::default();
    let explore = exploring(&asked);

    let refused = explore
        .traces(
            &SessionToken::from_plaintext("ct_ss_nope"),
            an_org(MINE),
            &a_filter(),
        )
        .await;

    assert_eq!(refused, Err(ExploreError::NoSession));
    assert!(asked.tenants.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn an_unreachable_span_store_is_not_reported_as_a_refusal() {
    let explore = exploring(Unreachable);

    let failed = explore
        .traces(
            &SessionToken::from_plaintext(TOKEN),
            an_org(MINE),
            &a_filter(),
        )
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
            &SessionToken::from_plaintext(TOKEN),
            an_org(THEIRS),
            window,
            None,
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
        .services(
            &SessionToken::from_plaintext(TOKEN),
            an_org(MINE),
            window,
            None,
        )
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
