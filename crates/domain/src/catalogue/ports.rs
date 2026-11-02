use core::future::Future;

use super::entities::Project;
use super::value_objects::{Assigned, Created, ProjectId, ProjectName, Renamed};
use crate::explore::ReadError;
use crate::organisation::TeamId;
use crate::telemetry::TenantId;

pub trait Projects {
    fn of(&self, tenant: TenantId) -> impl Future<Output = Result<Vec<Project>, ReadError>> + Send;

    fn create(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Created, ReadError>> + Send;

    fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Renamed, ReadError>> + Send;

    fn forget(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;

    fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> impl Future<Output = Result<Assigned, ReadError>> + Send;
}

impl<T: Projects + Sync> Projects for &T {
    fn of(&self, tenant: TenantId) -> impl Future<Output = Result<Vec<Project>, ReadError>> + Send {
        (*self).of(tenant)
    }

    fn create(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Created, ReadError>> + Send {
        (*self).create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Renamed, ReadError>> + Send {
        (*self).rename(org, project, name)
    }

    fn forget(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).forget(org, project)
    }

    fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> impl Future<Output = Result<Assigned, ReadError>> + Send {
        (*self).assign(org, project, team)
    }
}

impl<T: Projects + Send + Sync> Projects for std::sync::Arc<T> {
    fn of(&self, tenant: TenantId) -> impl Future<Output = Result<Vec<Project>, ReadError>> + Send {
        self.as_ref().of(tenant)
    }

    fn create(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Created, ReadError>> + Send {
        self.as_ref().create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Renamed, ReadError>> + Send {
        self.as_ref().rename(org, project, name)
    }

    fn forget(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().forget(org, project)
    }

    fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> impl Future<Output = Result<Assigned, ReadError>> + Send {
        self.as_ref().assign(org, project, team)
    }
}
