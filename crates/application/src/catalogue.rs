use ferristrace_domain::{
    Catalogue, CatalogueError, Project, ProjectId, ProjectName, SessionToken, TeamId, TenantId,
    Whoami,
};

use crate::registry::CatalogueRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: CatalogueRepos,
{
    #[transactional(sessions, memberships, projects, unavailable = CatalogueError::StoreUnreachable)]
    pub async fn list_projects(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<Project>, CatalogueError> {
        Catalogue::new(Whoami::new(sessions, memberships), projects)
            .projects(token, org)
            .await
    }

    #[transactional(sessions, memberships, projects, unavailable = CatalogueError::StoreUnreachable)]
    pub async fn create_project(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<Vec<Project>, CatalogueError> {
        Catalogue::new(Whoami::new(sessions, memberships), projects)
            .create(token, org, name)
            .await
    }

    #[transactional(sessions, memberships, projects, unavailable = CatalogueError::StoreUnreachable)]
    pub async fn rename_project(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> Result<Vec<Project>, CatalogueError> {
        Catalogue::new(Whoami::new(sessions, memberships), projects)
            .rename(token, org, project, name)
            .await
    }

    #[transactional(sessions, memberships, projects, unavailable = CatalogueError::StoreUnreachable)]
    pub async fn forget_project(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
    ) -> Result<Vec<Project>, CatalogueError> {
        Catalogue::new(Whoami::new(sessions, memberships), projects)
            .forget(token, org, project)
            .await
    }

    #[transactional(sessions, memberships, projects, unavailable = CatalogueError::StoreUnreachable)]
    pub async fn assign_project(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> Result<Vec<Project>, CatalogueError> {
        Catalogue::new(Whoami::new(sessions, memberships), projects)
            .assign(token, org, project, team)
            .await
    }
}
