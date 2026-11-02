use super::entities::{Holder, Member};
use super::errors::RosterError;
use super::ports::{Memberships, Roll};
use super::value_objects::{Invitation, OrgRole, Who};
use super::whoami::Whoami;
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target};
use crate::identity::{AccountId, Email, SessionStore};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Roster<S, G, M, A> {
    whoami: Whoami<S, G>,
    roll: M,
    trail: A,
}

impl<S, G, M, A> Roster<S, G, M, A>
where
    S: SessionStore,
    G: Memberships,
    M: Roll,
    A: AuditTrail,
{
    pub fn new(whoami: Whoami<S, G>, roll: M, trail: A) -> Self {
        Self {
            whoami,
            roll,
            trail,
        }
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
        let granted = self
            .roll
            .grant(org, account, role)
            .await?
            .ok_or(RosterError::NoSuchMember)?;
        self.note(
            org,
            &who,
            Action::MemberRoleGranted,
            account,
            Detail::default().with("from", held).with("to", role),
        )
        .await?;
        Ok(granted)
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
        if !self.roll.remove(org, account).await? {
            return Err(RosterError::NoSuchMember);
        }
        self.note(
            org,
            &who,
            Action::MemberRemoved,
            account,
            Detail::default().with("role", held),
        )
        .await
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
        let (holder, created) = if let Some(holder) = self.roll.standing_of(email).await? {
            (holder, false)
        } else {
            let account = self.roll.create_account(email).await?;
            let holder = Holder {
                account,
                name: None,
                photo: None,
            };
            (holder, true)
        };
        let joined = self
            .roll
            .join(org, holder.account, role)
            .await?
            .ok_or(RosterError::AlreadyAMember)?;
        self.note(
            org,
            &who,
            Action::MemberInvited,
            holder.account,
            Detail::default()
                .with("email", email.as_str())
                .with("role", joined.role),
        )
        .await?;
        Ok(Invitation {
            member: Member {
                account: holder.account,
                email: email.clone(),
                name: holder.name,
                photo: holder.photo,
                role: joined.role,
                joined_at_unix_nanos: joined.joined_at_unix_nanos,
            },
            created,
        })
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

    async fn note(
        &self,
        org: TenantId,
        who: &Who,
        action: Action,
        member: AccountId,
        detail: Detail,
    ) -> Result<(), RosterError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(who.account),
                action,
                target: Target::member(member),
                detail,
            })
            .await?)
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
