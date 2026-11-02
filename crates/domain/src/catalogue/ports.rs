use core::future::Future;

use super::entities::Project;
use super::value_objects::{Assigned, Created, ProjectId, ProjectName, Renamed};
use crate::organisation::TeamId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait Projects {
    fn of(&self, tenant: TenantId)
    -> impl Future<Output = Result<Vec<Project>, StoreError>> + Send;

    fn create(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Created, StoreError>> + Send;

    fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Renamed, StoreError>> + Send;

    fn forget(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> impl Future<Output = Result<Assigned, StoreError>> + Send;
}

impl<T: Projects + Sync> Projects for &T {
    fn of(
        &self,
        tenant: TenantId,
    ) -> impl Future<Output = Result<Vec<Project>, StoreError>> + Send {
        (*self).of(tenant)
    }

    fn create(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Created, StoreError>> + Send {
        (*self).create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Renamed, StoreError>> + Send {
        (*self).rename(org, project, name)
    }

    fn forget(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        (*self).forget(org, project)
    }

    fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> impl Future<Output = Result<Assigned, StoreError>> + Send {
        (*self).assign(org, project, team)
    }
}

impl<T: Projects + Send + Sync> Projects for std::sync::Arc<T> {
    fn of(
        &self,
        tenant: TenantId,
    ) -> impl Future<Output = Result<Vec<Project>, StoreError>> + Send {
        self.as_ref().of(tenant)
    }

    fn create(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Created, StoreError>> + Send {
        self.as_ref().create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Renamed, StoreError>> + Send {
        self.as_ref().rename(org, project, name)
    }

    fn forget(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        self.as_ref().forget(org, project)
    }

    fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> impl Future<Output = Result<Assigned, StoreError>> + Send {
        self.as_ref().assign(org, project, team)
    }
}
