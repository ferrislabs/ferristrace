use core::future::Future;

use super::value_objects::{Placements, Sighting};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait PlacementReader {
    fn placements(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Placements, StoreError>> + Send;

    fn sighting(
        &self,
        _tenant: TenantId,
        _service: &str,
        _window: Window,
        _project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Sighting, StoreError>> + Send {
        async { Ok(Sighting::default()) }
    }
}

impl<T: PlacementReader + Sync> PlacementReader for &T {
    fn placements(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Placements, StoreError>> + Send {
        (*self).placements(tenant, window, project)
    }

    fn sighting(
        &self,
        tenant: TenantId,
        service: &str,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Sighting, StoreError>> + Send {
        (*self).sighting(tenant, service, window, project)
    }
}

impl<T: PlacementReader + Send + Sync> PlacementReader for std::sync::Arc<T> {
    fn placements(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Placements, StoreError>> + Send {
        self.as_ref().placements(tenant, window, project)
    }

    fn sighting(
        &self,
        tenant: TenantId,
        service: &str,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Sighting, StoreError>> + Send {
        self.as_ref().sighting(tenant, service, window, project)
    }
}
