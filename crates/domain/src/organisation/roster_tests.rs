use std::sync::Mutex;
use uuid::Uuid;

use super::entities::{Holder, Joined, Member, Membership};
use super::errors::{RosterError, WhoamiError};
use super::ports::{Memberships, Roll};
use super::roster::Roster;
use super::value_objects::{Invitation, OrgRole};
use super::whoami::Whoami;
use crate::audit::{Action, Actor, Target};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Agent, Email, Session, SessionError, SessionStore};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::TenantId;
use crate::testing::Notebook;

const TOKEN: &str = "ft_ss_live";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const ME: u128 = 9;
const SOMEBODY: u128 = 10;
const JOINED: u64 = 1_700_000_000_000_000_000;

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn an_account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn an_address(local: &str) -> Email {
    Email::try_from(format!("{local}@example.com").as_str()).expect("an address")
}

fn a_member(seed: u128, role: OrgRole) -> Member {
    Member {
        account: an_account(seed),
        email: an_address(&format!("person{seed}")),
        name: None,
        photo: None,
        role,
        joined_at_unix_nanos: JOINED,
    }
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
                account: an_account(ME),
                email: an_address("me"),
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Written {
    granted: Vec<(AccountId, OrgRole)>,
    removed: Vec<AccountId>,
    admitted: Vec<(String, OrgRole)>,
    locked: usize,
}

#[derive(Debug)]
struct Held {
    roll: Mutex<Vec<Member>>,
    written: Mutex<Written>,
    holder: Option<Holder>,
    already_in: bool,
    asked: Mutex<Option<String>>,
}

impl Held {
    fn of(roll: Vec<Member>) -> Self {
        Self {
            roll: Mutex::new(roll),
            written: Mutex::default(),
            holder: None,
            already_in: false,
            asked: Mutex::default(),
        }
    }

    fn awaiting() -> Self {
        Self::of(Vec::new())
    }

    fn standing(account: AccountId) -> Self {
        Self {
            holder: Some(Holder {
                account,
                name: None,
                photo: None,
            }),
            ..Self::of(Vec::new())
        }
    }

    fn already_in(account: AccountId) -> Self {
        Self {
            already_in: true,
            ..Self::standing(account)
        }
    }

    fn written(&self) -> Written {
        self.written.lock().expect("the lock").clone()
    }

    fn roll(&self) -> Vec<Member> {
        self.roll.lock().expect("the lock").clone()
    }
}

impl Roll for &Held {
    async fn of(&self, _org: TenantId) -> Result<Vec<Member>, StoreError> {
        Ok(self.roll())
    }

    async fn grant(
        &self,
        _org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> Result<Option<Member>, StoreError> {
        self.written
            .lock()
            .expect("the lock")
            .granted
            .push((account, role));
        let mut held = self.roll.lock().expect("the lock");
        Ok(held
            .iter_mut()
            .find(|member| member.account == account)
            .map(|member| {
                member.role = role;
                member.clone()
            }))
    }

    async fn remove(&self, _org: TenantId, account: AccountId) -> Result<bool, StoreError> {
        self.written.lock().expect("the lock").removed.push(account);
        let mut roll = self.roll.lock().expect("the lock");
        let before = roll.len();
        roll.retain(|member| member.account != account);
        Ok(roll.len() < before)
    }

    async fn standing_of(&self, email: &Email) -> Result<Option<Holder>, StoreError> {
        *self.asked.lock().expect("the lock") = Some(email.as_str().to_owned());
        Ok(self.holder.clone())
    }

    async fn create_locked(&self, _email: &Email) -> Result<AccountId, StoreError> {
        self.written.lock().expect("the lock").locked += 1;
        Ok(an_account(SOMEBODY))
    }

    async fn join(
        &self,
        _org: TenantId,
        _account: AccountId,
        role: OrgRole,
    ) -> Result<Option<Joined>, StoreError> {
        let asked = self
            .asked
            .lock()
            .expect("the lock")
            .clone()
            .unwrap_or_default();
        self.written
            .lock()
            .expect("the lock")
            .admitted
            .push((asked, role));
        Ok((!self.already_in).then_some(Joined {
            role,
            joined_at_unix_nanos: JOINED,
        }))
    }
}

fn roster(role: OrgRole, held: &Held) -> Roster<OneSession, MemberOfMine, &Held, Notebook> {
    noting(role, held, &Notebook::default())
}

fn noting<'a>(
    role: OrgRole,
    held: &'a Held,
    notebook: &Notebook,
) -> Roster<OneSession, MemberOfMine, &'a Held, Notebook> {
    Roster::new(
        Whoami::new(OneSession, MemberOfMine(role)),
        held,
        notebook.clone(),
    )
}

fn token() -> SessionToken {
    SessionToken::from_plaintext(TOKEN)
}

#[tokio::test]
async fn a_member_reads_the_whole_roll_of_its_own_organisation() {
    let held = Held::of(vec![
        a_member(ME, OrgRole::Member),
        a_member(SOMEBODY, OrgRole::Admin),
    ]);

    let roll = roster(OrgRole::Member, &held)
        .members(&token(), an_org(MINE))
        .await
        .expect("a member may read");

    assert_eq!(roll.len(), 2);
    assert_eq!(roll[0].email, an_address("person9"));
    assert_eq!(roll[1].role, OrgRole::Admin);
}

#[tokio::test]
async fn the_roll_of_another_organisation_is_refused() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let refused = roster(OrgRole::Owner, &held)
        .members(&token(), an_org(THEIRS))
        .await;

    assert_eq!(refused, Err(RosterError::NotYours));
}

#[tokio::test]
async fn a_token_no_session_carries_reads_nothing() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let refused = roster(OrgRole::Owner, &held)
        .members(&SessionToken::from_plaintext("ft_ss_nope"), an_org(MINE))
        .await;

    assert_eq!(refused, Err(RosterError::NoSession));
}

#[tokio::test]
async fn an_administrator_changes_the_role_of_a_member_and_reads_it_back() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let changed = roster(OrgRole::Admin, &held)
        .grant(&token(), an_org(MINE), an_account(SOMEBODY), OrgRole::Admin)
        .await
        .expect("an administrator may change a member");

    assert_eq!(changed.role, OrgRole::Admin);
    assert_eq!(
        held.written().granted,
        vec![(an_account(SOMEBODY), OrgRole::Admin)]
    );
}

#[tokio::test]
async fn a_member_changes_nobody() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let refused = roster(OrgRole::Member, &held)
        .grant(&token(), an_org(MINE), an_account(SOMEBODY), OrgRole::Admin)
        .await;

    assert_eq!(refused, Err(RosterError::NotYoursToChange));
    assert!(held.written().granted.is_empty());
}

#[tokio::test]
async fn an_account_this_organisation_never_held_is_no_member_of_it() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let refused = roster(OrgRole::Owner, &held)
        .grant(&token(), an_org(MINE), an_account(404), OrgRole::Admin)
        .await;

    assert_eq!(refused, Err(RosterError::NoSuchMember));
    assert!(held.written().granted.is_empty());
}

#[tokio::test]
async fn an_administrator_cannot_promote_anybody_to_owner() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    let refused = roster(OrgRole::Admin, &held)
        .grant(&token(), an_org(MINE), an_account(SOMEBODY), OrgRole::Owner)
        .await;

    assert_eq!(refused, Err(RosterError::OwnersOnly));
    assert!(held.written().granted.is_empty());
}

#[tokio::test]
async fn an_administrator_cannot_demote_an_owner() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Owner)]);

    let refused = roster(OrgRole::Admin, &held)
        .grant(
            &token(),
            an_org(MINE),
            an_account(SOMEBODY),
            OrgRole::Member,
        )
        .await;

    assert_eq!(refused, Err(RosterError::OwnersOnly));
    assert!(held.written().granted.is_empty());
}

#[tokio::test]
async fn an_owner_hands_the_owner_role_to_somebody_else() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Admin)]);

    let changed = roster(OrgRole::Owner, &held)
        .grant(&token(), an_org(MINE), an_account(SOMEBODY), OrgRole::Owner)
        .await
        .expect("an owner may hand the role on");

    assert_eq!(changed.role, OrgRole::Owner);
}

#[tokio::test]
async fn the_last_owner_cannot_stop_being_one() {
    let held = Held::of(vec![a_member(ME, OrgRole::Owner)]);

    let refused = roster(OrgRole::Owner, &held)
        .grant(&token(), an_org(MINE), an_account(ME), OrgRole::Member)
        .await;

    assert_eq!(refused, Err(RosterError::LastOwner));
    assert!(held.written().granted.is_empty());
}

#[tokio::test]
async fn an_owner_steps_down_once_somebody_else_answers_for_the_organisation() {
    let held = Held::of(vec![
        a_member(ME, OrgRole::Owner),
        a_member(SOMEBODY, OrgRole::Owner),
    ]);

    let changed = roster(OrgRole::Owner, &held)
        .grant(&token(), an_org(MINE), an_account(ME), OrgRole::Admin)
        .await
        .expect("an owner may step down while another one stays");

    assert_eq!(changed.role, OrgRole::Admin);
}

#[tokio::test]
async fn an_owner_hands_the_role_on_and_then_leaves() {
    let held = Held::of(vec![
        a_member(ME, OrgRole::Owner),
        a_member(SOMEBODY, OrgRole::Admin),
    ]);
    let settling = roster(OrgRole::Owner, &held);

    settling
        .grant(&token(), an_org(MINE), an_account(SOMEBODY), OrgRole::Owner)
        .await
        .expect("the role is handed on");
    settling
        .remove(&token(), an_org(MINE), an_account(ME))
        .await
        .expect("and then the organisation can be left");

    assert_eq!(held.roll().len(), 1);
    assert_eq!(held.roll()[0].role, OrgRole::Owner);
}

#[tokio::test]
async fn an_administrator_may_leave_an_organisation_it_does_not_own() {
    let held = Held::of(vec![
        a_member(ME, OrgRole::Admin),
        a_member(SOMEBODY, OrgRole::Owner),
    ]);

    roster(OrgRole::Admin, &held)
        .remove(&token(), an_org(MINE), an_account(ME))
        .await
        .expect("leaving is a change to your own membership and nobody else");

    assert_eq!(held.written().removed, vec![an_account(ME)]);
}

#[tokio::test]
async fn an_administrator_shows_a_member_the_door() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);

    roster(OrgRole::Admin, &held)
        .remove(&token(), an_org(MINE), an_account(SOMEBODY))
        .await
        .expect("an administrator may remove a member");

    assert_eq!(held.written().removed, vec![an_account(SOMEBODY)]);
    assert!(held.roll().is_empty());
}

#[tokio::test]
async fn an_administrator_cannot_remove_an_owner() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Owner)]);

    let refused = roster(OrgRole::Admin, &held)
        .remove(&token(), an_org(MINE), an_account(SOMEBODY))
        .await;

    assert_eq!(refused, Err(RosterError::OwnersOnly));
    assert!(held.written().removed.is_empty());
}

#[tokio::test]
async fn the_last_owner_cannot_leave_either() {
    let held = Held::of(vec![
        a_member(ME, OrgRole::Owner),
        a_member(SOMEBODY, OrgRole::Admin),
    ]);

    let refused = roster(OrgRole::Owner, &held)
        .remove(&token(), an_org(MINE), an_account(ME))
        .await;

    assert_eq!(
        refused,
        Err(RosterError::LastOwner),
        "an administrator left behind could not hand the role back out"
    );
    assert!(held.written().removed.is_empty());
}

#[tokio::test]
async fn an_unknown_address_is_admitted_and_an_account_is_created_for_it() {
    let held = Held::awaiting();

    let Invitation { member } = roster(OrgRole::Admin, &held)
        .invite(&token(), an_org(MINE), &an_address("new"), OrgRole::Member)
        .await
        .expect("an administrator may invite");

    assert_eq!(member.role, OrgRole::Member);
    assert_eq!(held.written().locked, 1);
    assert_eq!(
        held.written().admitted,
        vec![("new@example.com".to_owned(), OrgRole::Member)]
    );
}

#[tokio::test]
async fn an_address_that_already_has_an_account_joins_without_creating_one() {
    let held = Held::standing(an_account(SOMEBODY));

    let Invitation { member } = roster(OrgRole::Owner, &held)
        .invite(&token(), an_org(MINE), &an_address("known"), OrgRole::Admin)
        .await
        .expect("an owner may invite");

    assert_eq!(member.role, OrgRole::Admin);
    assert_eq!(held.written().locked, 0);
}

#[tokio::test]
async fn an_address_already_on_the_roll_is_refused_rather_than_regranted() {
    let held = Held::already_in(an_account(SOMEBODY));

    let refused = roster(OrgRole::Owner, &held)
        .invite(
            &token(),
            an_org(MINE),
            &an_address("already"),
            OrgRole::Member,
        )
        .await;

    assert_eq!(refused.err(), Some(RosterError::AlreadyAMember));
}

#[tokio::test]
async fn an_administrator_cannot_invite_an_owner() {
    let held = Held::awaiting();

    let refused = roster(OrgRole::Admin, &held)
        .invite(&token(), an_org(MINE), &an_address("new"), OrgRole::Owner)
        .await;

    assert_eq!(refused.err(), Some(RosterError::OwnersOnly));
    assert!(
        held.written().admitted.is_empty(),
        "no account may be created for a role the inviter cannot hand out"
    );
}

#[tokio::test]
async fn inviting_into_an_organisation_this_session_is_no_part_of_says_no_such_thing() {
    let held = Held::awaiting();

    let refused = roster(OrgRole::Owner, &held)
        .invite(
            &token(),
            an_org(THEIRS),
            &an_address("new"),
            OrgRole::Member,
        )
        .await;

    assert_eq!(refused.err(), Some(RosterError::NotYours));
    assert!(held.written().admitted.is_empty());
}

#[tokio::test]
async fn inviting_an_address_with_an_account_adds_a_membership_and_creates_nothing() {
    let held = Held::standing(an_account(SOMEBODY));

    let Invitation { member } = roster(OrgRole::Owner, &held)
        .invite(&token(), an_org(MINE), &an_address("known"), OrgRole::Admin)
        .await
        .expect("an owner may invite");

    assert_eq!(member.account, an_account(SOMEBODY));
    assert_eq!(member.email, an_address("known"));
    assert_eq!(member.role, OrgRole::Admin);
    assert_eq!(member.joined_at_unix_nanos, JOINED);
    let written = held.written();
    assert_eq!(
        written.admitted,
        vec![("known@example.com".to_owned(), OrgRole::Admin)]
    );
    assert_eq!(written.locked, 0, "no account may be created");
}

#[tokio::test]
async fn inviting_a_new_address_creates_one_locked_account() {
    let held = Held::awaiting();

    let Invitation { member } = roster(OrgRole::Admin, &held)
        .invite(&token(), an_org(MINE), &an_address("new"), OrgRole::Member)
        .await
        .expect("an administrator may invite");

    assert_eq!(member.account, an_account(SOMEBODY));
    assert_eq!(member.name, None);
    let written = held.written();
    assert_eq!(written.locked, 1);
    assert_eq!(
        written.admitted,
        vec![("new@example.com".to_owned(), OrgRole::Member)]
    );
}

#[tokio::test]
async fn an_address_already_on_the_roll_creates_no_account() {
    let held = Held::already_in(an_account(SOMEBODY));

    let refused = roster(OrgRole::Owner, &held)
        .invite(
            &token(),
            an_org(MINE),
            &an_address("already"),
            OrgRole::Member,
        )
        .await;

    assert_eq!(refused.err(), Some(RosterError::AlreadyAMember));
    let written = held.written();
    assert_eq!(written.locked, 0);
}

#[tokio::test]
async fn changing_a_role_records_who_changed_whom_from_what_to_what() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);
    let notebook = Notebook::default();

    noting(OrgRole::Admin, &held, &notebook)
        .grant(&token(), an_org(MINE), an_account(SOMEBODY), OrgRole::Admin)
        .await
        .expect("an administrator may change a member");

    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::MemberRoleGranted);
    assert_eq!(entries[0].actor, Actor::Account(an_account(ME)));
    assert_eq!(entries[0].target, Target::member(an_account(SOMEBODY)));
    assert_eq!(entries[0].detail.get("from"), Some(Some("member")));
    assert_eq!(entries[0].detail.get("to"), Some(Some("admin")));
}

#[tokio::test]
async fn a_refused_role_change_records_nothing() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);
    let notebook = Notebook::default();

    let refused = noting(OrgRole::Admin, &held, &notebook)
        .grant(&token(), an_org(MINE), an_account(SOMEBODY), OrgRole::Owner)
        .await;

    assert_eq!(refused, Err(RosterError::OwnersOnly));
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn removing_a_member_records_the_role_it_held() {
    let held = Held::of(vec![a_member(SOMEBODY, OrgRole::Member)]);
    let notebook = Notebook::default();

    noting(OrgRole::Admin, &held, &notebook)
        .remove(&token(), an_org(MINE), an_account(SOMEBODY))
        .await
        .expect("an administrator may remove a member");

    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::MemberRemoved);
    assert_eq!(entries[0].target, Target::member(an_account(SOMEBODY)));
    assert_eq!(entries[0].detail.get("role"), Some(Some("member")));
}

#[tokio::test]
async fn inviting_records_the_address_and_role() {
    let held = Held::awaiting();
    let notebook = Notebook::default();

    let invitation = noting(OrgRole::Admin, &held, &notebook)
        .invite(&token(), an_org(MINE), &an_address("new"), OrgRole::Member)
        .await
        .expect("an administrator may invite");

    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::MemberInvited);
    assert_eq!(entries[0].target, Target::member(invitation.member.account));
    assert_eq!(
        entries[0].detail.get("email"),
        Some(Some("new@example.com"))
    );
    assert_eq!(entries[0].detail.get("role"), Some(Some("member")));
}
