use ferristrace_domain::{
    AccountId, ProjectId, SessionToken, Squad, Team, TeamError, TeamId, TeamName, TenantId, Whoami,
};

use crate::registry::TeamRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: TeamRepos,
{
    #[transactional(sessions, memberships, teams, audit, unavailable = TeamError::StoreUnreachable)]
    pub async fn list_teams(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<Team>, TeamError> {
        Squad::new(Whoami::new(sessions, memberships), teams, audit)
            .teams(token, org)
            .await
    }

    #[transactional(sessions, memberships, teams, audit, unavailable = TeamError::StoreUnreachable)]
    pub async fn create_team(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &TeamName,
    ) -> Result<Vec<Team>, TeamError> {
        Squad::new(Whoami::new(sessions, memberships), teams, audit)
            .create(token, org, name)
            .await
    }

    #[transactional(sessions, memberships, teams, audit, unavailable = TeamError::StoreUnreachable)]
    pub async fn rename_team(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> Result<Vec<Team>, TeamError> {
        Squad::new(Whoami::new(sessions, memberships), teams, audit)
            .rename(token, org, team, name)
            .await
    }

    #[transactional(sessions, memberships, teams, audit, unavailable = TeamError::StoreUnreachable)]
    pub async fn disband_team(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
    ) -> Result<Vec<Team>, TeamError> {
        Squad::new(Whoami::new(sessions, memberships), teams, audit)
            .disband(token, org, team)
            .await
    }

    #[transactional(sessions, memberships, teams, audit, unavailable = TeamError::StoreUnreachable)]
    pub async fn seat_team(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> Result<Vec<Team>, TeamError> {
        Squad::new(Whoami::new(sessions, memberships), teams, audit)
            .seat(token, org, team, members)
            .await
    }

    #[transactional(sessions, memberships, teams, audit, unavailable = TeamError::StoreUnreachable)]
    pub async fn own_projects(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> Result<Vec<Team>, TeamError> {
        Squad::new(Whoami::new(sessions, memberships), teams, audit)
            .own(token, org, team, projects)
            .await
    }
}
