use super::entities::Team;
use super::errors::TeamError;
use super::ports::{Memberships, Squads};
use super::value_objects::{Named, OrgRole, TeamId, TeamName};
use super::whoami::Whoami;
use crate::catalogue::ProjectId;
use crate::identity::{AccountId, SessionStore};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Squad<S, G, Q> {
    whoami: Whoami<S, G>,
    squads: Q,
}

impl<S, G, Q> Squad<S, G, Q>
where
    S: SessionStore,
    G: Memberships,
    Q: Squads,
{
    pub fn new(whoami: Whoami<S, G>, squads: Q) -> Self {
        Self { whoami, squads }
    }

    pub async fn teams(&self, token: &SessionToken, org: TenantId) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Member).await?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn create(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &TeamName,
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        admitted(self.squads.create(org, name).await?)?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn rename(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        admitted(self.squads.rename(org, team, name).await?)?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn disband(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        found(self.squads.disband(org, team).await?)?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn seat(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        found(self.squads.seat(org, team, members).await?)?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn own(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        found(self.squads.own(org, team, projects).await?)?;
        Ok(self.squads.of(org).await?)
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
        floor: OrgRole,
    ) -> Result<(), TeamError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(TeamError::NotYours),
            Some(role) if !role.at_least(floor) => Err(TeamError::NotYoursToChange),
            Some(_) => Ok(()),
        }
    }
}

fn admitted(named: Named) -> Result<(), TeamError> {
    match named {
        Named::Free => Ok(()),
        Named::Taken => Err(TeamError::NameTaken),
    }
}

fn found(there: bool) -> Result<(), TeamError> {
    if there {
        Ok(())
    } else {
        Err(TeamError::NoSuchTeam)
    }
}
