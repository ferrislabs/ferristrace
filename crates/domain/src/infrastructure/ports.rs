use core::future::Future;

use super::entities::Resource;
use super::value_objects::InfraFilter;
use crate::explore::Window;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait InfraReader {
    fn resources(
        &self,
        org: TenantId,
        figures: Window,
        filter: &InfraFilter,
    ) -> impl Future<Output = Result<Vec<Resource>, StoreError>> + Send;
}

impl<T: InfraReader + Sync> InfraReader for &T {
    fn resources(
        &self,
        org: TenantId,
        figures: Window,
        filter: &InfraFilter,
    ) -> impl Future<Output = Result<Vec<Resource>, StoreError>> + Send {
        (*self).resources(org, figures, filter)
    }
}

impl<T: InfraReader + Send + Sync> InfraReader for std::sync::Arc<T> {
    fn resources(
        &self,
        org: TenantId,
        figures: Window,
        filter: &InfraFilter,
    ) -> impl Future<Output = Result<Vec<Resource>, StoreError>> + Send {
        self.as_ref().resources(org, figures, filter)
    }
}
