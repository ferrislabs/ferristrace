use core::future::Future;

use super::value_objects::{ServiceSummary, TraceFilter, TraceSummary, Window};
use crate::catalogue::ProjectId;
use crate::store::StoreError;
use crate::telemetry::{Span, TenantId, TraceId};

pub trait SpanReader {
    fn services(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, StoreError>> + Send;

    fn traces(
        &self,
        tenant: TenantId,
        filter: &TraceFilter,
    ) -> impl Future<Output = Result<Vec<TraceSummary>, StoreError>> + Send;

    fn trace(
        &self,
        tenant: TenantId,
        id: TraceId,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<Span>, StoreError>> + Send;
}

impl<T: SpanReader + Sync> SpanReader for &T {
    fn services(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, StoreError>> + Send {
        (*self).services(tenant, window, project)
    }

    fn traces(
        &self,
        tenant: TenantId,
        filter: &TraceFilter,
    ) -> impl Future<Output = Result<Vec<TraceSummary>, StoreError>> + Send {
        (*self).traces(tenant, filter)
    }

    fn trace(
        &self,
        tenant: TenantId,
        id: TraceId,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<Span>, StoreError>> + Send {
        (*self).trace(tenant, id, project)
    }
}

impl<T: SpanReader + Send + Sync> SpanReader for std::sync::Arc<T> {
    fn services(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, StoreError>> + Send {
        self.as_ref().services(tenant, window, project)
    }

    fn traces(
        &self,
        tenant: TenantId,
        filter: &TraceFilter,
    ) -> impl Future<Output = Result<Vec<TraceSummary>, StoreError>> + Send {
        self.as_ref().traces(tenant, filter)
    }

    fn trace(
        &self,
        tenant: TenantId,
        id: TraceId,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<Span>, StoreError>> + Send {
        self.as_ref().trace(tenant, id, project)
    }
}
