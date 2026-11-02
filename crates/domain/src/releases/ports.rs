use core::future::Future;

use super::entities::{Deploy, Deployed, Release, ReleaseDetail};
use super::value_objects::ReleaseFilter;
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait ReleaseReader {
    fn releases(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> impl Future<Output = Result<Vec<Release>, StoreError>> + Send;

    fn release(
        &self,
        org: TenantId,
        service: &str,
        version: &str,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Option<ReleaseDetail>, StoreError>> + Send;

    fn deploys(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> impl Future<Output = Result<Vec<Deploy>, StoreError>> + Send;
}

impl<T: ReleaseReader + Sync> ReleaseReader for &T {
    fn releases(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> impl Future<Output = Result<Vec<Release>, StoreError>> + Send {
        (*self).releases(org, filter)
    }

    fn release(
        &self,
        org: TenantId,
        service: &str,
        version: &str,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Option<ReleaseDetail>, StoreError>> + Send {
        (*self).release(org, service, version, project)
    }

    fn deploys(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> impl Future<Output = Result<Vec<Deploy>, StoreError>> + Send {
        (*self).deploys(org, filter)
    }
}

pub trait DeployFeed {
    fn deployed(
        &self,
        window: Window,
    ) -> impl Future<Output = Result<Vec<Deployed>, StoreError>> + Send;
}

impl<T: DeployFeed + Sync> DeployFeed for &T {
    fn deployed(
        &self,
        window: Window,
    ) -> impl Future<Output = Result<Vec<Deployed>, StoreError>> + Send {
        (*self).deployed(window)
    }
}

pub trait DeployMarks {
    fn watermark(&self) -> impl Future<Output = Result<Option<u64>, StoreError>> + Send;

    fn advance(&self, to_ms: u64) -> impl Future<Output = Result<(), StoreError>> + Send;
}
