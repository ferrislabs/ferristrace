use core::future::Future;

use super::jobs::{JobGrouping, JobSeries, JobSummary};
use super::query::AttributeKey;
use super::requests::{RequestGrouping, RequestSeries, RequestSummary};
use super::statements::{StatementGrouping, StatementSeries, StatementSummary};
use super::value_objects::{
    AttributeFilter, CacheGrouping, CacheSeries, CacheSummary, FacetFilter, Facets, ListedSpan,
    SeenAttribute, SeenValue, SeriesFilter, SeriesResult, ServiceFilter, ServiceSeries,
    ServiceSummary, SpanFilter, TableFilter, TableRow, TraceFilter, TraceSummary,
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

    fn span_table(
        &self,
        _tenant: TenantId,
        _filter: &TableFilter,
    ) -> impl Future<Output = Result<Vec<TableRow>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn caches(
        &self,
        _tenant: TenantId,
        _grouping: CacheGrouping,
        _filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<CacheSummary>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn cache_series(
        &self,
        _tenant: TenantId,
        _filter: &ServiceFilter,
    ) -> impl Future<Output = Result<CacheSeries, StoreError>> + Send {
        async { Ok(CacheSeries::default()) }
    }

    fn requests(
        &self,
        _tenant: TenantId,
        _grouping: RequestGrouping,
        _filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<RequestSummary>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn request_series(
        &self,
        _tenant: TenantId,
        _filter: &ServiceFilter,
    ) -> impl Future<Output = Result<RequestSeries, StoreError>> + Send {
        async { Ok(RequestSeries::default()) }
    }

    fn jobs(
        &self,
        _tenant: TenantId,
        _grouping: JobGrouping,
        _filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<JobSummary>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn job_series(
        &self,
        _tenant: TenantId,
        _filter: &ServiceFilter,
    ) -> impl Future<Output = Result<JobSeries, StoreError>> + Send {
        async { Ok(JobSeries::default()) }
    }

    fn statements(
        &self,
        _tenant: TenantId,
        _grouping: StatementGrouping,
        _filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<StatementSummary>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn statement_series(
        &self,
        _tenant: TenantId,
        _filter: &ServiceFilter,
    ) -> impl Future<Output = Result<StatementSeries, StoreError>> + Send {
        async { Ok(StatementSeries::default()) }
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

    fn span_table(
        &self,
        tenant: TenantId,
        filter: &TableFilter,
    ) -> impl Future<Output = Result<Vec<TableRow>, StoreError>> + Send {
        (*self).span_table(tenant, filter)
    }

    fn caches(
        &self,
        tenant: TenantId,
        grouping: CacheGrouping,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<CacheSummary>, StoreError>> + Send {
        (*self).caches(tenant, grouping, filter)
    }

    fn cache_series(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<CacheSeries, StoreError>> + Send {
        (*self).cache_series(tenant, filter)
    }

    fn requests(
        &self,
        tenant: TenantId,
        grouping: RequestGrouping,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<RequestSummary>, StoreError>> + Send {
        (*self).requests(tenant, grouping, filter)
    }

    fn request_series(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<RequestSeries, StoreError>> + Send {
        (*self).request_series(tenant, filter)
    }

    fn jobs(
        &self,
        tenant: TenantId,
        grouping: JobGrouping,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<JobSummary>, StoreError>> + Send {
        (*self).jobs(tenant, grouping, filter)
    }

    fn job_series(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<JobSeries, StoreError>> + Send {
        (*self).job_series(tenant, filter)
    }

    fn statements(
        &self,
        tenant: TenantId,
        grouping: StatementGrouping,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<StatementSummary>, StoreError>> + Send {
        (*self).statements(tenant, grouping, filter)
    }

    fn statement_series(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<StatementSeries, StoreError>> + Send {
        (*self).statement_series(tenant, filter)
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

    fn span_table(
        &self,
        tenant: TenantId,
        filter: &TableFilter,
    ) -> impl Future<Output = Result<Vec<TableRow>, StoreError>> + Send {
        self.as_ref().span_table(tenant, filter)
    }

    fn caches(
        &self,
        tenant: TenantId,
        grouping: CacheGrouping,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<CacheSummary>, StoreError>> + Send {
        self.as_ref().caches(tenant, grouping, filter)
    }

    fn cache_series(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<CacheSeries, StoreError>> + Send {
        self.as_ref().cache_series(tenant, filter)
    }

    fn requests(
        &self,
        tenant: TenantId,
        grouping: RequestGrouping,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<RequestSummary>, StoreError>> + Send {
        self.as_ref().requests(tenant, grouping, filter)
    }

    fn request_series(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<RequestSeries, StoreError>> + Send {
        self.as_ref().request_series(tenant, filter)
    }

    fn jobs(
        &self,
        tenant: TenantId,
        grouping: JobGrouping,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<JobSummary>, StoreError>> + Send {
        self.as_ref().jobs(tenant, grouping, filter)
    }

    fn job_series(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<JobSeries, StoreError>> + Send {
        self.as_ref().job_series(tenant, filter)
    }

    fn statements(
        &self,
        tenant: TenantId,
        grouping: StatementGrouping,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<Vec<StatementSummary>, StoreError>> + Send {
        self.as_ref().statements(tenant, grouping, filter)
    }

    fn statement_series(
        &self,
        tenant: TenantId,
        filter: &ServiceFilter,
    ) -> impl Future<Output = Result<StatementSeries, StoreError>> + Send {
        self.as_ref().statement_series(tenant, filter)
    }
}
