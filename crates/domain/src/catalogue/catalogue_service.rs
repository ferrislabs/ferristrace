use super::entities::Project;
use super::errors::CatalogueError;
use super::ports::Projects;
use super::value_objects::{Assigned, Created, ProjectId, ProjectName, Renamed};
use crate::identity::SessionStore;
use crate::organisation::{Memberships, OrgRole, TeamId, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Catalogue<S, G, P> {
    whoami: Whoami<S, G>,
    projects: P,
}

impl<S, G, P> Catalogue<S, G, P>
where
    S: SessionStore,
    G: Memberships,
    P: Projects,
{
    pub fn new(whoami: Whoami<S, G>, projects: P) -> Self {
        Self { whoami, projects }
    }

    pub async fn projects(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Member).await?;
        Ok(self.projects.of(org).await?)
    }

    pub async fn create(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Admin).await?;
        match self.projects.create(org, name).await? {
            Created::Done => self.listed(org).await,
            Created::Taken => Err(CatalogueError::NameTaken),
        }
    }

    pub async fn rename(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Admin).await?;
        match self.projects.rename(org, project, name).await? {
            Renamed::Done => self.listed(org).await,
            Renamed::Taken => Err(CatalogueError::NameTaken),
            Renamed::Gone => Err(CatalogueError::NoSuchProject),
        }
    }

    pub async fn forget(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Admin).await?;
        if self.projects.forget(org, project).await? {
            self.listed(org).await
        } else {
            Err(CatalogueError::NoSuchProject)
        }
    }

    pub async fn assign(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Admin).await?;
        match self.projects.assign(org, project, team).await? {
            Assigned::Done => self.listed(org).await,
            Assigned::Gone => Err(CatalogueError::NoSuchProject),
            Assigned::NoSuchTeam => Err(CatalogueError::NoSuchTeam),
        }
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
        floor: OrgRole,
    ) -> Result<(), CatalogueError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(CatalogueError::NotYours),
            Some(role) if !role.at_least(floor) => Err(CatalogueError::NotYoursToChange),
            Some(_) => Ok(()),
        }
    }

    async fn listed(&self, org: TenantId) -> Result<Vec<Project>, CatalogueError> {
        Ok(self.projects.of(org).await?)
    }
}
