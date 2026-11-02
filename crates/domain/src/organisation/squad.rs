use super::entities::Team;
use super::errors::TeamError;
use super::ports::{Memberships, Squads};
use super::value_objects::{Named, OrgRole, TeamId, TeamName};
use super::whoami::Whoami;
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::catalogue::ProjectId;
use crate::identity::{AccountId, SessionStore};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Squad<S, G, Q, A> {
    whoami: Whoami<S, G>,
    squads: Q,
    trail: A,
}

impl<S, G, Q, A> Squad<S, G, Q, A>
where
    S: SessionStore,
    G: Memberships,
    Q: Squads,
    A: AuditTrail,
{
    pub fn new(whoami: Whoami<S, G>, squads: Q, trail: A) -> Self {
        Self {
            whoami,
            squads,
            trail,
        }
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
        let by = self.standing(token, org, OrgRole::Admin).await?;
        admitted(self.squads.create(org, name).await?)?;
        let teams = self.squads.of(org).await?;
        let made = teams
            .iter()
            .find(|team| &*team.name == name.as_str())
            .map(|team| team.id);
        let target = made.map_or_else(|| Target::of(TargetKind::Team, None), Target::team);
        self.note(
            org,
            by,
            Action::TeamCreated,
            target,
            Detail::default().with("name", name.as_str()),
        )
        .await?;
        Ok(teams)
    }

    pub async fn rename(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> Result<Vec<Team>, TeamError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        let before = self.name_of(org, team).await?;
        admitted(self.squads.rename(org, team, name).await?)?;
        self.note(
            org,
            by,
            Action::TeamRenamed,
            Target::team(team),
            Detail::default()
                .with_maybe("from", before)
                .with("to", name.as_str()),
        )
        .await?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn disband(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
    ) -> Result<Vec<Team>, TeamError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        let before = self.name_of(org, team).await?;
        found(self.squads.disband(org, team).await?)?;
        self.note(
            org,
            by,
            Action::TeamDisbanded,
            Target::team(team),
            Detail::default().with_maybe("name", before),
        )
        .await?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn seat(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> Result<Vec<Team>, TeamError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        found(self.squads.seat(org, team, members).await?)?;
        self.note(
            org,
            by,
            Action::TeamSeated,
            Target::team(team),
            Detail::default().with_all("members", members),
        )
        .await?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn own(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> Result<Vec<Team>, TeamError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        found(self.squads.own(org, team, projects).await?)?;
        self.note(
            org,
            by,
            Action::TeamProjectsOwned,
            Target::team(team),
            Detail::default().with_all("projects", projects),
        )
        .await?;
        Ok(self.squads.of(org).await?)
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
        floor: OrgRole,
    ) -> Result<AccountId, TeamError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(TeamError::NotYours),
            Some(role) if !role.at_least(floor) => Err(TeamError::NotYoursToChange),
            Some(_) => Ok(who.account),
        }
    }

    async fn name_of(&self, org: TenantId, team: TeamId) -> Result<Option<Box<str>>, TeamError> {
        Ok(self
            .squads
            .of(org)
            .await?
            .into_iter()
            .find(|held| held.id == team)
            .map(|held| held.name))
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        target: Target,
        detail: Detail,
    ) -> Result<(), TeamError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(by),
                action,
                target,
                detail,
            })
            .await?)
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
