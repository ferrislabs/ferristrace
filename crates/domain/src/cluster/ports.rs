use core::future::Future;

use super::entities::ClusterSnapshot;
use super::value_objects::ClusterFilter;
use crate::explore::Window;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait ClusterReader {
    fn snapshot(
        &self,
        org: TenantId,
        window: Window,
        filter: &ClusterFilter,
    ) -> impl Future<Output = Result<ClusterSnapshot, StoreError>> + Send;
}

impl<T: ClusterReader + Sync> ClusterReader for &T {
    fn snapshot(
        &self,
        org: TenantId,
        window: Window,
        filter: &ClusterFilter,
    ) -> impl Future<Output = Result<ClusterSnapshot, StoreError>> + Send {
        (*self).snapshot(org, window, filter)
    }
}

impl<T: ClusterReader + Send + Sync> ClusterReader for std::sync::Arc<T> {
    fn snapshot(
        &self,
        org: TenantId,
        window: Window,
        filter: &ClusterFilter,
    ) -> impl Future<Output = Result<ClusterSnapshot, StoreError>> + Send {
        self.as_ref().snapshot(org, window, filter)
    }
}
