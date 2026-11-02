use core::future::Future;

use super::entities::{Metric, MetricDescriptor};
use super::value_objects::{Aggregation, MetricKind, MetricQuery, SeriesSet};
use crate::catalogue::ProjectId;
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
}
