use core::future::Future;

use uuid::Uuid;

use super::entities::{Snapshot, SpanCursor, SpanPage};
use super::errors::ExportError;
use super::export_service::Exporter;
use super::ports::ExportSource;
use crate::audit::{Action, Actor, Target};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Agent, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, Organisation, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::{Span, SpanId, SpanKind, SpanStatus, TenantId, TraceId};
use crate::testing::Notebook;

const TOKEN: &str = "ft_ss_live";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const ME: u128 = 9;

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
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
                account: AccountId::new(Uuid::from_u128(ME)),
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

struct MemberOfMine(OrgRole);

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: an_org(MINE),
            name: "Acme".into(),
            role: self.0,
        }])
    }
}

struct Source {
    reachable: bool,
}

impl ExportSource for Source {
    fn snapshot(&self, org: TenantId) -> impl Future<Output = Result<Snapshot, StoreError>> + Send {
        let reachable = self.reachable;
        async move {
            if !reachable {
                return Err(StoreError::Unreachable);
            }
            Ok(Snapshot {
                org: Organisation {
                    id: org,
                    name: "Acme".into(),
                    plan: "free".into(),
                    created_at_unix_nanos: 0,
                    logo: None,
                    retention: None,
                    retention_cap: None,
                },
                projects: Vec::new(),
                teams: Vec::new(),
                members: Vec::new(),
                keys: Vec::new(),
                decisions: Vec::new(),
                audit: Vec::new(),
            })
        }
    }
}

fn exporter(
    role: OrgRole,
    reachable: bool,
    notebook: &Notebook,
) -> Exporter<OneSession, MemberOfMine, Source, Notebook> {
    Exporter::new(
        Whoami::new(OneSession, MemberOfMine(role)),
        Source { reachable },
        notebook.clone(),
    )
}

fn token() -> SessionToken {
    SessionToken::from_plaintext(TOKEN)
}

fn at() -> UnixSeconds {
    UnixSeconds::new(1_700_000_000)
}

#[tokio::test]
async fn an_owner_is_handed_the_snapshot_of_their_organisation() {
    let notebook = Notebook::default();

    let prepared = exporter(OrgRole::Owner, true, &notebook)
        .begin(&token(), an_org(MINE), at())
        .await
        .expect("an owner exports");

    assert_eq!(prepared.grant.org(), an_org(MINE));
    assert_eq!(prepared.snapshot.org.id, an_org(MINE));
    assert_eq!(prepared.exported_at, at());
}

#[tokio::test]
async fn an_export_is_audited_with_the_account_that_asked() {
    let notebook = Notebook::default();

    exporter(OrgRole::Owner, true, &notebook)
        .begin(&token(), an_org(MINE), at())
        .await
        .expect("an owner exports");

    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].org, an_org(MINE));
    assert_eq!(entries[0].action, Action::OrgExported);
    assert_eq!(
        entries[0].actor,
        Actor::Account(AccountId::new(Uuid::from_u128(ME)))
    );
    assert_eq!(entries[0].target, Target::org(an_org(MINE)));
}

#[tokio::test]
async fn an_admin_and_a_member_are_refused_and_nothing_is_audited() {
    for role in [OrgRole::Admin, OrgRole::Member] {
        let notebook = Notebook::default();

        let refused = exporter(role, true, &notebook)
            .begin(&token(), an_org(MINE), at())
            .await;

        assert_eq!(refused.err(), Some(ExportError::OwnersOnly));
        assert!(notebook.entries().is_empty());
    }
}

#[tokio::test]
async fn a_stranger_is_refused_another_organisations_export() {
    let notebook = Notebook::default();

    let refused = exporter(OrgRole::Owner, true, &notebook)
        .begin(&token(), an_org(THEIRS), at())
        .await;

    assert_eq!(refused.err(), Some(ExportError::NotYours));
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn an_unknown_session_is_refused() {
    let notebook = Notebook::default();

    let refused = exporter(OrgRole::Owner, true, &notebook)
        .begin(
            &SessionToken::from_plaintext("ft_ss_other"),
            an_org(MINE),
            at(),
        )
        .await;

    assert_eq!(refused.err(), Some(ExportError::NoSession));
}

#[tokio::test]
async fn an_unreachable_store_is_reported() {
    let notebook = Notebook::default();

    let refused = exporter(OrgRole::Owner, false, &notebook)
        .begin(&token(), an_org(MINE), at())
        .await;

    assert_eq!(refused.err(), Some(ExportError::StoreUnreachable));
}

fn a_span(seed: u8) -> super::entities::ExportedSpan {
    super::entities::ExportedSpan {
        project: None,
        span: Span {
            trace_id: TraceId::try_from([seed; 16].as_slice()).expect("sixteen bytes"),
            span_id: SpanId::try_from([seed; 8].as_slice()).expect("eight bytes"),
            parent: None,
            name: "work".into(),
            service_name: "checkout".into(),
            kind: SpanKind::Server,
            status: SpanStatus::Ok,
            start_unix_nanos: u64::from(seed),
            end_unix_nanos: u64::from(seed) + 1,
            attributes: std::collections::BTreeMap::new(),
            events: Vec::new(),
        },
    }
}

#[test]
fn a_page_holding_one_row_too_many_hands_back_a_cursor_on_its_last_kept_row() {
    let page = SpanPage::cut(vec![a_span(1), a_span(2), a_span(3)], 2);

    assert_eq!(page.spans.len(), 2);
    assert_eq!(page.next, Some(SpanCursor::after(&a_span(2).span)));
}

#[test]
fn a_page_that_is_not_over_full_is_the_last() {
    let page = SpanPage::cut(vec![a_span(1), a_span(2)], 2);

    assert_eq!(page.spans.len(), 2);
    assert_eq!(page.next, None);
}
