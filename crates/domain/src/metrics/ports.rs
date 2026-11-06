use core::future::Future;

use super::entities::{Metric, MetricDescriptor};
use super::value_objects::{Aggregation, MetricKind, MetricQuery, SeriesSet};
use crate::catalogue::ProjectId;
use crate::explore::{AttributeFilter, AttributeKey, SeenAttribute, SeenValue};
use crate::store::StoreError;
use crate::telemetry::{Scope, SinkError, TenantId};

pub trait MetricSink {
    fn accept_metrics(
        &self,
        scope: Scope,
        metrics: Vec<Metric>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send;
}

impl<T: MetricSink + Sync> MetricSink for &T {
    fn accept_metrics(
        &self,
        scope: Scope,
        metrics: Vec<Metric>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        (*self).accept_metrics(scope, metrics)
    }
}

impl<T: MetricSink + Send + Sync> MetricSink for std::sync::Arc<T> {
    fn accept_metrics(
        &self,
        scope: Scope,
        metrics: Vec<Metric>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        self.as_ref().accept_metrics(scope, metrics)
    }
}

pub trait MetricReader {
    fn catalogue(
        &self,
        org: TenantId,
        search: Option<&str>,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<MetricDescriptor>, StoreError>> + Send;

    fn series(
        &self,
        org: TenantId,
        kind: MetricKind,
        aggregation: Aggregation,
        query: &MetricQuery,
    ) -> impl Future<Output = Result<SeriesSet, StoreError>> + Send;

    fn attributes(
        &self,
        _org: TenantId,
        _metric: &str,
        _filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenAttribute>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn attribute_values(
        &self,
        _org: TenantId,
        _metric: &str,
        _key: &AttributeKey,
        _filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenValue>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }
}

impl<T: MetricReader + Sync> MetricReader for &T {
    fn catalogue(
        &self,
        org: TenantId,
        search: Option<&str>,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<MetricDescriptor>, StoreError>> + Send {
        (*self).catalogue(org, search, project)
    }

    fn series(
        &self,
        org: TenantId,
        kind: MetricKind,
        aggregation: Aggregation,
        query: &MetricQuery,
    ) -> impl Future<Output = Result<SeriesSet, StoreError>> + Send {
        (*self).series(org, kind, aggregation, query)
    }

    fn attributes(
        &self,
        org: TenantId,
        metric: &str,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenAttribute>, StoreError>> + Send {
        (*self).attributes(org, metric, filter)
    }

    fn attribute_values(
        &self,
        org: TenantId,
        metric: &str,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenValue>, StoreError>> + Send {
        (*self).attribute_values(org, metric, key, filter)
    }
}

impl<T: MetricReader + Send + Sync> MetricReader for std::sync::Arc<T> {
    fn catalogue(
        &self,
        org: TenantId,
        search: Option<&str>,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<MetricDescriptor>, StoreError>> + Send {
        self.as_ref().catalogue(org, search, project)
    }

    fn series(
        &self,
        org: TenantId,
        kind: MetricKind,
        aggregation: Aggregation,
        query: &MetricQuery,
    ) -> impl Future<Output = Result<SeriesSet, StoreError>> + Send {
        self.as_ref().series(org, kind, aggregation, query)
    }

    fn attributes(
        &self,
        org: TenantId,
        metric: &str,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenAttribute>, StoreError>> + Send {
        self.as_ref().attributes(org, metric, filter)
    }

    fn attribute_values(
        &self,
        org: TenantId,
        metric: &str,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenValue>, StoreError>> + Send {
        self.as_ref().attribute_values(org, metric, key, filter)
    }
}
