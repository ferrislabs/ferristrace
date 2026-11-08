use std::sync::Mutex;

use uuid::Uuid;

use super::desk::InboxDesk;
use super::entities::{InboxItem, InboxItemDraft};
use super::errors::InboxError;
use super::ports::InboxStore;
use super::value_objects::{InboxItemId, InboxItemKind};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::TenantId;

const TOKEN: &str = "ft_ss_live";
const ACCOUNT: u128 = 9;
const ORG: u128 = 1;
const OTHER_ORG: u128 = 2;

fn account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn token() -> SessionToken {
    SessionToken::for_subject(TOKEN)
}

#[derive(Default)]
struct Ledger {
    items: Mutex<Vec<InboxItem>>,
}

impl InboxStore for &Ledger {
    async fn list(
        &self,
        acc: AccountId,
        asked: TenantId,
        limit: u32,
    ) -> Result<Vec<InboxItem>, StoreError> {
        Ok(self
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
        let item = InboxItem {
            id: InboxItemId::fresh(),
            account: draft.account,
            org: draft.org,
            kind: draft.kind,
            title: draft.title.clone(),
            body: draft.body.clone(),
            link: draft.link.clone(),
            created_ms: at_ms,
            read_ms: None,
        };
        self.items.lock().expect("the lock").push(item.clone());
        Ok(item)
    }

    async fn mark_read(
        &self,
        acc: AccountId,
        asked: TenantId,
        id: InboxItemId,
        at_ms: u64,
    ) -> Result<bool, StoreError> {
        let mut items = self.items.lock().expect("the lock");
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
        let mut items = self.items.lock().expect("the lock");
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

    async fn prune(&self, older_than_ms: u64) -> Result<u64, StoreError> {
        let mut items = self.items.lock().expect("the lock");
        let before = items.len();
        items.retain(|item| item.created_ms >= older_than_ms);
        Ok((before - items.len()) as u64)
    }
}

struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, asked: &SessionToken) -> Result<Session, SessionError> {
        if asked.identity().subject.as_str() != TOKEN {
            return Err(SessionError::Unknown);
        }
        Ok(Session {
            account: account(ACCOUNT),
            email: Email::try_from("person@example.com").expect("an address"),
            name: None,
            photo: None,
            org: None,
            expires_at: UnixSeconds::new(0),
        })
    }
}

struct MemberOfOrg;

impl Memberships for MemberOfOrg {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: org(ORG),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

fn desk(ledger: &Ledger) -> InboxDesk<OneSession, MemberOfOrg, &Ledger> {
    InboxDesk::new(Whoami::new(OneSession, MemberOfOrg), ledger)
}

fn draft(kind: InboxItemKind) -> InboxItemDraft {
    InboxItemDraft {
        account: account(ACCOUNT),
        org: org(ORG),
        kind,
        title: "Title".into(),
        body: "Body".into(),
        link: None,
    }
}

#[tokio::test]
async fn an_item_raised_for_an_account_is_listed_unread() {
    let ledger = Ledger::default();
    let desk = desk(&ledger);
    desk.notify(&draft(InboxItemKind::Membership), 1_000)
        .await
        .expect("raised");

    let items = desk.list(&token(), org(ORG)).await.expect("a read");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, InboxItemKind::Membership);
    assert_eq!(items[0].read_ms, None);
}

#[tokio::test]
async fn a_stranger_to_the_org_cannot_list_its_items() {
    let ledger = Ledger::default();
    let desk = desk(&ledger);

    let refused = desk.list(&token(), org(OTHER_ORG)).await;

    assert_eq!(refused.err(), Some(InboxError::NotYours));
}

#[tokio::test]
async fn marking_one_item_read_sets_when_and_leaves_the_rest_alone() {
    let ledger = Ledger::default();
    let desk = desk(&ledger);
    let raised = desk
        .notify(&draft(InboxItemKind::Issue), 1_000)
        .await
        .expect("raised");
    desk.notify(&draft(InboxItemKind::Usage), 1_000)
        .await
        .expect("raised");

    desk.mark_read(&token(), org(ORG), raised.id, 2_000)
        .await
        .expect("marked");

    let items = desk.list(&token(), org(ORG)).await.expect("a read");
    let read: Vec<_> = items.iter().filter(|item| item.read_ms.is_some()).collect();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].id, raised.id);
    assert_eq!(read[0].read_ms, Some(2_000));
}

#[tokio::test]
async fn marking_an_unknown_item_is_refused() {
    let ledger = Ledger::default();
    let desk = desk(&ledger);

    let refused = desk
        .mark_read(&token(), org(ORG), InboxItemId::fresh(), 2_000)
        .await;

    assert_eq!(refused.err(), Some(InboxError::NoSuchItem));
}

#[tokio::test]
async fn mark_all_read_flips_every_unread_item_and_counts_them() {
    let ledger = Ledger::default();
    let desk = desk(&ledger);
    desk.notify(&draft(InboxItemKind::Issue), 1_000)
        .await
        .expect("raised");
    desk.notify(&draft(InboxItemKind::Usage), 1_000)
        .await
        .expect("raised");

    let flipped = desk
        .mark_all_read(&token(), org(ORG), 2_000)
        .await
        .expect("marked");

    assert_eq!(flipped, 2);
    let items = desk.list(&token(), org(ORG)).await.expect("a read");
    assert!(items.iter().all(|item| item.read_ms == Some(2_000)));
}

#[tokio::test]
async fn a_token_with_no_session_is_refused() {
    let ledger = Ledger::default();
    let desk = desk(&ledger);

    let refused = desk
        .list(&SessionToken::for_subject("nope"), org(ORG))
        .await;

    assert_eq!(refused.err(), Some(InboxError::NoSession));
}
