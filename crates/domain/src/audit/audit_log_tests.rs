use uuid::Uuid;

use super::audit_log::AuditLog;
use super::entities::AuditEntry;
use super::errors::AuditError;
use super::value_objects::{Action, Actor, AuditFilter, Detail, PageSize, Target};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, WhoamiError};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;
use crate::testing::Notebook;

const TOKEN: &str = "ft_ss_live";
const MINE: u128 = 1;
const THEIRS: u128 = 2;

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn an_account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
            Ok(Session {
                account: an_account(9),
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

fn log(role: OrgRole, notebook: &Notebook) -> AuditLog<OneSession, MemberOfMine, Notebook> {
    AuditLog::new(
        crate::organisation::Whoami::new(OneSession, MemberOfMine(role)),
        notebook.clone(),
    )
}

fn token() -> SessionToken {
    SessionToken::for_subject(TOKEN)
}

fn renamed(org: TenantId, to: &str) -> AuditEntry {
    AuditEntry {
        org,
        actor: Actor::Account(an_account(9)),
        action: Action::OrgRenamed,
        target: Target::org(org),
        detail: Detail::default().with("to", to),
    }
}

async fn noted(notebook: &Notebook, entries: &[AuditEntry]) {
    use super::ports::AuditTrail;
    for entry in entries {
        notebook.record(entry).await.expect("a noted entry");
    }
}

#[tokio::test]
async fn an_admin_and_an_owner_read_the_log_newest_first() {
    let notebook = Notebook::default();
    noted(
        &notebook,
        &[renamed(an_org(MINE), "One"), renamed(an_org(MINE), "Two")],
    )
    .await;

    for role in [OrgRole::Admin, OrgRole::Owner] {
        let page = log(role, &notebook)
            .read(
                &token(),
                an_org(MINE),
                &AuditFilter::default(),
                PageSize::new(None),
            )
            .await
            .expect("an administrator reads the log");

        let names: Vec<_> = page
            .entries
            .iter()
            .map(|logged| logged.entry.detail.get("to").flatten())
            .collect();
        assert_eq!(names, vec![Some("Two"), Some("One")]);
        assert_eq!(page.next, None);
    }
}

#[tokio::test]
async fn a_member_is_refused_the_log() {
    let notebook = Notebook::default();
    noted(&notebook, &[renamed(an_org(MINE), "One")]).await;

    let refused = log(OrgRole::Member, &notebook)
        .read(
            &token(),
            an_org(MINE),
            &AuditFilter::default(),
            PageSize::new(None),
        )
        .await;

    assert_eq!(refused.err(), Some(AuditError::NotYoursToRead));
}

#[tokio::test]
async fn a_stranger_is_refused_another_organisations_log() {
    let notebook = Notebook::default();
    noted(&notebook, &[renamed(an_org(THEIRS), "Theirs")]).await;

    let refused = log(OrgRole::Owner, &notebook)
        .read(
            &token(),
            an_org(THEIRS),
            &AuditFilter::default(),
            PageSize::new(None),
        )
        .await;

    assert_eq!(refused.err(), Some(AuditError::NotYours));
}

#[tokio::test]
async fn an_unknown_session_is_refused() {
    let notebook = Notebook::default();

    let refused = log(OrgRole::Owner, &notebook)
        .read(
            &SessionToken::for_subject("ft_ss_other"),
            an_org(MINE),
            &AuditFilter::default(),
            PageSize::new(None),
        )
        .await;

    assert_eq!(refused.err(), Some(AuditError::NoSession));
}

#[tokio::test]
async fn a_full_page_names_the_cursor_that_reads_the_rest() {
    let notebook = Notebook::default();
    noted(
        &notebook,
        &[
            renamed(an_org(MINE), "One"),
            renamed(an_org(MINE), "Two"),
            renamed(an_org(MINE), "Three"),
        ],
    )
    .await;
    let reading = log(OrgRole::Admin, &notebook);

    let first = reading
        .read(
            &token(),
            an_org(MINE),
            &AuditFilter::default(),
            PageSize::new(Some(2)),
        )
        .await
        .expect("a first page");
    let rest = reading
        .read(
            &token(),
            an_org(MINE),
            &AuditFilter {
                before: first.next,
                ..AuditFilter::default()
            },
            PageSize::new(Some(2)),
        )
        .await
        .expect("the rest");

    assert_eq!(first.entries.len(), 2);
    assert_eq!(
        first.next,
        first.entries.last().map(super::entities::Logged::cursor)
    );
    assert_eq!(rest.entries.len(), 1);
    assert_eq!(rest.entries[0].entry.detail.get("to"), Some(Some("One")));
    assert_eq!(rest.next, None);
}
