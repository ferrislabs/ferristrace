use super::entities::Member;
use super::errors::RosterError;
use super::ports::{Memberships, Roll};
use super::value_objects::{Admitted, Invitation, OrgRole, Who};
use super::whoami::Whoami;
use crate::identity::{AccountId, Email, SessionStore};
use crate::secret::{ResetToken, SessionToken};
use crate::telemetry::TenantId;

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
