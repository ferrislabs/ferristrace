use super::entities::Project;
use super::errors::CatalogueError;
use super::ports::Projects;
use super::value_objects::{Assigned, Created, ProjectId, ProjectName, Renamed};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, OrgRole, TeamId, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Catalogue<S, G, P, A> {
    whoami: Whoami<S, G>,
    projects: P,
    trail: A,
}

impl<S, G, P, A> Catalogue<S, G, P, A>
where
    S: SessionStore,
    G: Memberships,
    P: Projects,
    A: AuditTrail,
{
    pub fn new(whoami: Whoami<S, G>, projects: P, trail: A) -> Self {
        Self {
            whoami,
            projects,
            trail,
        }
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
        let by = self.standing(token, org, OrgRole::Admin).await?;
        if self.projects.create(org, name).await? == Created::Taken {
            return Err(CatalogueError::NameTaken);
        }
        let listed = self.listed(org).await?;
        let target = listed
            .iter()
            .find(|project| &*project.name == name.as_str())
            .map_or_else(
                || Target::of(TargetKind::Project, None),
                |project| Target::project(project.id),
            );
        self.note(
            org,
            by,
            Action::ProjectCreated,
            target,
            Detail::default().with("name", name.as_str()),
        )
        .await?;
        Ok(listed)
    }

    pub async fn rename(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> Result<Vec<Project>, CatalogueError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        let before = self.name_of(org, project).await?;
        match self.projects.rename(org, project, name).await? {
            Renamed::Done => {}
            Renamed::Taken => return Err(CatalogueError::NameTaken),
            Renamed::Gone => return Err(CatalogueError::NoSuchProject),
        }
        self.note(
            org,
            by,
            Action::ProjectRenamed,
            Target::project(project),
            Detail::default()
                .with_maybe("from", before)
                .with("to", name.as_str()),
        )
        .await?;
        self.listed(org).await
    }

    pub async fn forget(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
    ) -> Result<Vec<Project>, CatalogueError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        let before = self.name_of(org, project).await?;
        if !self.projects.forget(org, project).await? {
            return Err(CatalogueError::NoSuchProject);
        }
        self.note(
            org,
            by,
            Action::ProjectDeleted,
            Target::project(project),
            Detail::default().with_maybe("name", before),
        )
        .await?;
        self.listed(org).await
    }

    pub async fn assign(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> Result<Vec<Project>, CatalogueError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        match self.projects.assign(org, project, team).await? {
            Assigned::Done => {}
            Assigned::Gone => return Err(CatalogueError::NoSuchProject),
            Assigned::NoSuchTeam => return Err(CatalogueError::NoSuchTeam),
        }
        self.note(
            org,
            by,
            Action::ProjectAssigned,
            Target::project(project),
            Detail::default().with_maybe("team", team),
        )
        .await?;
        self.listed(org).await
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
        floor: OrgRole,
    ) -> Result<AccountId, CatalogueError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(CatalogueError::NotYours),
            Some(role) if !role.at_least(floor) => Err(CatalogueError::NotYoursToChange),
            Some(_) => Ok(who.account),
        }
    }

    async fn name_of(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> Result<Option<Box<str>>, CatalogueError> {
        Ok(self
            .listed(org)
            .await?
            .into_iter()
            .find(|held| held.id == project)
            .map(|held| held.name))
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        target: Target,
        detail: Detail,
    ) -> Result<(), CatalogueError> {
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

    async fn listed(&self, org: TenantId) -> Result<Vec<Project>, CatalogueError> {
        Ok(self.projects.of(org).await?)
    }
}
