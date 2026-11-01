use core::future::Future;

use crate::credential::Email;
use crate::explore::ReadError;
use crate::identity::AccountId;
use crate::membership::{Memberships, OrgRole, Who, Whoami, WhoamiError};
use crate::photo::Portrait;
use crate::secret::{ResetToken, SecretHash, SessionToken};
use crate::session::SessionStore;
use crate::tenant::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub account: AccountId,
    pub email: Email,
    pub name: Option<Box<str>>,
    pub photo: Option<Portrait>,
    pub role: OrgRole,
    pub joined_at_unix_nanos: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admitted {
    Awaiting(Member),
    Standing(Member),
    AlreadyIn,
}

pub trait Roll {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Member>, ReadError>> + Send;

    fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Member>, ReadError>> + Send;

    fn remove(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;

    fn admit(
        &self,
        org: TenantId,
        email: &Email,
        role: OrgRole,
        way_in: SecretHash,
    ) -> impl Future<Output = Result<Admitted, ReadError>> + Send;
}

impl<T: Roll + Sync> Roll for &T {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Member>, ReadError>> + Send {
        (*self).of(org)
    }

    fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Member>, ReadError>> + Send {
        (*self).grant(org, account, role)
    }

    fn remove(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).remove(org, account)
    }

    fn admit(
        &self,
        org: TenantId,
        email: &Email,
        role: OrgRole,
        way_in: SecretHash,
    ) -> impl Future<Output = Result<Admitted, ReadError>> + Send {
        (*self).admit(org, email, role, way_in)
    }
}

impl<T: Roll + Send + Sync> Roll for std::sync::Arc<T> {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Member>, ReadError>> + Send {
        self.as_ref().of(org)
    }

    fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> impl Future<Output = Result<Option<Member>, ReadError>> + Send {
        self.as_ref().grant(org, account, role)
    }

    fn remove(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().remove(org, account)
    }

    fn admit(
        &self,
        org: TenantId,
        email: &Email,
        role: OrgRole,
        way_in: SecretHash,
    ) -> impl Future<Output = Result<Admitted, ReadError>> + Send {
        self.as_ref().admit(org, email, role, way_in)
    }
}

#[derive(Debug)]
pub struct Invitation {
    pub member: Member,
    pub way_in: Option<ResetToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RosterError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("changing who belongs here is for an administrator to do")]
    NotYoursToChange,
    #[error("the owner role is an owner to hand out and an owner to take back")]
    OwnersOnly,
    #[error("an organisation keeps at least one owner")]
    LastOwner,
    #[error("no member of this organisation carries that account")]
    NoSuchMember,
    #[error("that address already belongs to this organisation")]
    AlreadyAMember,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for RosterError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadError> for RosterError {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug)]
pub struct Roster<S, G, M> {
    whoami: Whoami<S, G>,
    roll: M,
}

impl<S, G, M> Roster<S, G, M>
where
    S: SessionStore,
    G: Memberships,
    M: Roll,
{
    pub fn new(whoami: Whoami<S, G>, roll: M) -> Self {
        Self { whoami, roll }
    }

    pub async fn members(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<Member>, RosterError> {
        self.standing(token, org, OrgRole::Member).await?;
        Ok(self.roll.of(org).await?)
    }

    pub async fn grant(
        &self,
        token: &SessionToken,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> Result<Member, RosterError> {
        let who = self.standing(token, org, OrgRole::Admin).await?;
        let (held, owners) = self.standing_in(org, account).await?;
        owners_only(&who, org, [held, role])?;
        keeps_an_owner(held, Some(role), owners)?;
        self.roll
            .grant(org, account, role)
            .await?
            .ok_or(RosterError::NoSuchMember)
    }

    pub async fn remove(
        &self,
        token: &SessionToken,
        org: TenantId,
        account: AccountId,
    ) -> Result<(), RosterError> {
        let who = self.standing(token, org, OrgRole::Admin).await?;
        let (held, owners) = self.standing_in(org, account).await?;
        owners_only(&who, org, [held])?;
        keeps_an_owner(held, None, owners)?;
        if self.roll.remove(org, account).await? {
            Ok(())
        } else {
            Err(RosterError::NoSuchMember)
        }
    }

    pub async fn invite(
        &self,
        token: &SessionToken,
        org: TenantId,
        email: &Email,
        role: OrgRole,
    ) -> Result<Invitation, RosterError> {
        let who = self.standing(token, org, OrgRole::Admin).await?;
        owners_only(&who, org, [role])?;
        let way_in = ResetToken::random();
        match self.roll.admit(org, email, role, way_in.hash()).await? {
            Admitted::Awaiting(member) => Ok(Invitation {
                member,
                way_in: Some(way_in),
            }),
            Admitted::Standing(member) => Ok(Invitation {
                member,
                way_in: None,
            }),
            Admitted::AlreadyIn => Err(RosterError::AlreadyAMember),
        }
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
        floor: OrgRole,
    ) -> Result<Who, RosterError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(RosterError::NotYours),
            Some(role) if !role.at_least(floor) => Err(RosterError::NotYoursToChange),
            Some(_) => Ok(who),
        }
    }

    async fn standing_in(
        &self,
        org: TenantId,
        account: AccountId,
    ) -> Result<(OrgRole, usize), RosterError> {
        let roll = self.roll.of(org).await?;
        let held = roll
            .iter()
            .find(|member| member.account == account)
            .map(|member| member.role)
            .ok_or(RosterError::NoSuchMember)?;
        Ok((held, owners_among(&roll)))
    }
}

fn owners_among(roll: &[Member]) -> usize {
    roll.iter()
        .filter(|member| member.role == OrgRole::Owner)
        .count()
}

fn keeps_an_owner(
    held: OrgRole,
    wanted: Option<OrgRole>,
    owners: usize,
) -> Result<(), RosterError> {
    let staying = wanted == Some(OrgRole::Owner);
    if held == OrgRole::Owner && !staying && owners <= 1 {
        return Err(RosterError::LastOwner);
    }
    Ok(())
}

fn owners_only<const N: usize>(
    who: &Who,
    org: TenantId,
    touched: [OrgRole; N],
) -> Result<(), RosterError> {
    if touched.contains(&OrgRole::Owner) && who.role_in(org) != Some(OrgRole::Owner) {
        return Err(RosterError::OwnersOnly);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use uuid::Uuid;

    use super::{Admitted, Invitation, Member, Roll, Roster, RosterError};
    use crate::clock::UnixSeconds;
    use crate::credential::Email;
    use crate::explore::ReadError;
    use crate::identity::AccountId;
    use crate::membership::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
    use crate::secret::{SecretHash, SessionToken};
    use crate::session::{Agent, Session, SessionError, SessionStore};
    use crate::tenant::TenantId;

    const TOKEN: &str = "ct_ss_live";
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
    }

    #[derive(Debug)]
    struct Held {
        roll: Mutex<Vec<Member>>,
        written: Mutex<Written>,
        admission: Option<Admitted>,
    }

    impl Held {
        fn of(roll: Vec<Member>) -> Self {
            Self {
                roll: Mutex::new(roll),
                written: Mutex::default(),
                admission: None,
            }
        }

        fn admitting(admission: Admitted) -> Self {
            Self {
                roll: Mutex::new(Vec::new()),
                written: Mutex::default(),
                admission: Some(admission),
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
        async fn of(&self, _org: TenantId) -> Result<Vec<Member>, ReadError> {
            Ok(self.roll())
        }

        async fn grant(
            &self,
            _org: TenantId,
            account: AccountId,
            role: OrgRole,
        ) -> Result<Option<Member>, ReadError> {
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

        async fn remove(&self, _org: TenantId, account: AccountId) -> Result<bool, ReadError> {
            self.written.lock().expect("the lock").removed.push(account);
            let mut roll = self.roll.lock().expect("the lock");
            let before = roll.len();
            roll.retain(|member| member.account != account);
            Ok(roll.len() < before)
        }

        async fn admit(
            &self,
            _org: TenantId,
            email: &Email,
            role: OrgRole,
            _way_in: SecretHash,
        ) -> Result<Admitted, ReadError> {
            self.written
                .lock()
                .expect("the lock")
                .admitted
                .push((email.as_str().to_owned(), role));
            Ok(self.admission.clone().unwrap_or(Admitted::AlreadyIn))
        }
    }

    fn roster(role: OrgRole, held: &Held) -> Roster<OneSession, MemberOfMine, &Held> {
        Roster::new(Whoami::new(OneSession, MemberOfMine(role)), held)
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
            .members(&SessionToken::from_plaintext("ct_ss_nope"), an_org(MINE))
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
    async fn an_unknown_address_is_admitted_with_a_way_in_of_its_own() {
        let held = Held::admitting(Admitted::Awaiting(a_member(SOMEBODY, OrgRole::Member)));

        let Invitation { member, way_in } = roster(OrgRole::Admin, &held)
            .invite(&token(), an_org(MINE), &an_address("new"), OrgRole::Member)
            .await
            .expect("an administrator may invite");

        assert_eq!(member.role, OrgRole::Member);
        let way_in = way_in.expect("a new account cannot sign in without one");
        assert!(way_in.looks_like_one(), "{way_in:?}");
        assert_eq!(
            held.written().admitted,
            vec![("new@example.com".to_owned(), OrgRole::Member)]
        );
    }

    #[tokio::test]
    async fn an_address_that_already_has_an_account_joins_without_a_new_way_in() {
        let held = Held::admitting(Admitted::Standing(a_member(SOMEBODY, OrgRole::Admin)));

        let Invitation { member, way_in } = roster(OrgRole::Owner, &held)
            .invite(&token(), an_org(MINE), &an_address("known"), OrgRole::Admin)
            .await
            .expect("an owner may invite");

        assert_eq!(member.role, OrgRole::Admin);
        assert!(way_in.is_none());
    }

    #[tokio::test]
    async fn an_address_already_on_the_roll_is_refused_rather_than_regranted() {
        let held = Held::admitting(Admitted::AlreadyIn);

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
        let held = Held::admitting(Admitted::Awaiting(a_member(SOMEBODY, OrgRole::Owner)));

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
        let held = Held::admitting(Admitted::Awaiting(a_member(SOMEBODY, OrgRole::Member)));

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
}
