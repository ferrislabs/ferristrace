use core::future::Future;

use super::query::AttributeKey;
use super::value_objects::{
    AttributeFilter, FacetFilter, Facets, ListedSpan, SeenAttribute, SeenValue, SeriesFilter,
    SeriesResult, ServiceFilter, ServiceSeries, ServiceSummary, SpanFilter, TraceFilter,
    TraceSummary,
};
use crate::catalogue::ProjectId;
use crate::store::StoreError;
use crate::telemetry::{Span, TenantId, TraceId};

pub trait SpanReader {
    fn services(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, StoreError>> + Send;

    fn operations(
        &self,
        tenant: TenantId,
        service: &str,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, StoreError>> + Send;

    fn series(
        &self,
        tenant: TenantId,
        service: Option<&str>,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<ServiceSeries, StoreError>> + Send;

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

    fn attributes(
        &self,
        _tenant: TenantId,
        _filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenAttribute>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn attribute_values(
        &self,
        _tenant: TenantId,
        _key: &AttributeKey,
        _filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenValue>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn facets(
        &self,
        _tenant: TenantId,
        _filter: &FacetFilter,
    ) -> impl Future<Output = Result<Facets, StoreError>> + Send {
        async { Ok(Facets::default()) }
    }

    fn spans(
        &self,
        _tenant: TenantId,
        _filter: &SpanFilter,
    ) -> impl Future<Output = Result<Vec<ListedSpan>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn span_series(
        &self,
        _tenant: TenantId,
        _filter: &SeriesFilter,
    ) -> impl Future<Output = Result<SeriesResult, StoreError>> + Send {
        async { Ok(SeriesResult::default()) }
    }
}

impl<T: SpanReader + Sync> SpanReader for &T {
    fn services(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, StoreError>> + Send {
        (*self).services(tenant, filter)
    }

    fn operations(
        &self,
        tenant: TenantId,
        service: &str,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, StoreError>> + Send {
        (*self).operations(tenant, service, filter)
    }

    fn series(
        &self,
        tenant: TenantId,
        service: Option<&str>,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<ServiceSeries, StoreError>> + Send {
        (*self).series(tenant, service, filter)
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

    fn attributes(
        &self,
        tenant: TenantId,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenAttribute>, StoreError>> + Send {
        (*self).attributes(tenant, filter)
    }

    fn attribute_values(
        &self,
        tenant: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenValue>, StoreError>> + Send {
        (*self).attribute_values(tenant, key, filter)
    }

    fn facets(
        &self,
        tenant: TenantId,
        filter: &FacetFilter,
    ) -> impl Future<Output = Result<Facets, StoreError>> + Send {
        (*self).facets(tenant, filter)
    }

    fn spans(
        &self,
        tenant: TenantId,
        filter: &SpanFilter,
    ) -> impl Future<Output = Result<Vec<ListedSpan>, StoreError>> + Send {
        (*self).spans(tenant, filter)
    }

    fn span_series(
        &self,
        tenant: TenantId,
        filter: &SeriesFilter,
    ) -> impl Future<Output = Result<SeriesResult, StoreError>> + Send {
        (*self).span_series(tenant, filter)
    }
}

impl<T: SpanReader + Send + Sync> SpanReader for std::sync::Arc<T> {
    fn services(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, StoreError>> + Send {
        self.as_ref().services(tenant, filter)
    }

    fn operations(
        &self,
        tenant: TenantId,
        service: &str,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<ServiceSummary>, StoreError>> + Send {
        self.as_ref().operations(tenant, service, filter)
    }

    fn series(
        &self,
        tenant: TenantId,
        service: Option<&str>,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<ServiceSeries, StoreError>> + Send {
        self.as_ref().series(tenant, service, filter)
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

    fn attributes(
        &self,
        tenant: TenantId,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenAttribute>, StoreError>> + Send {
        self.as_ref().attributes(tenant, filter)
    }

    fn attribute_values(
        &self,
        tenant: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenValue>, StoreError>> + Send {
        self.as_ref().attribute_values(tenant, key, filter)
    }

    fn facets(
        &self,
        tenant: TenantId,
        filter: &FacetFilter,
    ) -> impl Future<Output = Result<Facets, StoreError>> + Send {
        self.as_ref().facets(tenant, filter)
    }

    fn spans(
        &self,
        tenant: TenantId,
        filter: &SpanFilter,
    ) -> impl Future<Output = Result<Vec<ListedSpan>, StoreError>> + Send {
        self.as_ref().spans(tenant, filter)
    }

    fn span_series(
        &self,
        tenant: TenantId,
        filter: &SeriesFilter,
    ) -> impl Future<Output = Result<SeriesResult, StoreError>> + Send {
        self.as_ref().span_series(tenant, filter)
    }
}
