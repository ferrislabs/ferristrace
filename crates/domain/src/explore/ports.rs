use core::future::Future;

use super::errors::ReadError;
use super::value_objects::{ServiceSummary, TraceFilter, TraceSummary, Window};
use crate::catalogue::ProjectId;
use crate::telemetry::{Span, TenantId, TraceId};

pub trait SpanReader {
    fn services(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, ReadError>> + Send;

    fn traces(
        &self,
        tenant: TenantId,
        filter: &TraceFilter,
    ) -> impl Future<Output = Result<Vec<TraceSummary>, ReadError>> + Send;

    fn trace(
        &self,
        tenant: TenantId,
        id: TraceId,
    ) -> impl Future<Output = Result<Vec<Span>, ReadError>> + Send;
}

impl<T: SpanReader + Sync> SpanReader for &T {
    fn services(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, ReadError>> + Send {
        (*self).services(tenant, window, project)
    }

    fn traces(
        &self,
        tenant: TenantId,
        filter: &TraceFilter,
    ) -> impl Future<Output = Result<Vec<TraceSummary>, ReadError>> + Send {
        (*self).traces(tenant, filter)
    }

    fn trace(
        &self,
        tenant: TenantId,
        id: TraceId,
    ) -> impl Future<Output = Result<Vec<Span>, ReadError>> + Send {
        (*self).trace(tenant, id)
    }
}

impl<T: SpanReader + Send + Sync> SpanReader for std::sync::Arc<T> {
    fn services(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, ReadError>> + Send {
        self.as_ref().services(tenant, window, project)
    }

    fn traces(
        &self,
        tenant: TenantId,
        filter: &TraceFilter,
    ) -> impl Future<Output = Result<Vec<TraceSummary>, ReadError>> + Send {
        self.as_ref().traces(tenant, filter)
    }

    fn trace(
        &self,
        tenant: TenantId,
        id: TraceId,
    ) -> impl Future<Output = Result<Vec<Span>, ReadError>> + Send {
        self.as_ref().trace(tenant, id)
    }
}
