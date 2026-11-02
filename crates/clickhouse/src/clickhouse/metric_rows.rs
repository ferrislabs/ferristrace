use std::collections::BTreeMap;

use clickhouse::Row;
use ferristrace_domain::{
    Exemplar, HistogramPoint, Metric, MetricKind, MetricPoint, ProjectId, Scope, SinkError,
    Temporality, UnixSeconds,
};
use serde::Serialize;
use serde_repr::Serialize_repr;
use uuid::Uuid;

use crate::clickhouse::rows::{borrowed, nanos, seconds};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr)]
#[repr(i8)]
pub(crate) enum PointKindRow {
    Gauge = 1,
    Sum = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr)]
#[repr(i8)]
pub(crate) enum CatalogueKindRow {
    Gauge = 1,
    Sum = 2,
    Histogram = 3,
}

impl From<MetricKind> for CatalogueKindRow {
    fn from(kind: MetricKind) -> Self {
        match kind {
            MetricKind::Gauge => Self::Gauge,
            MetricKind::Sum => Self::Sum,
            MetricKind::Histogram => Self::Histogram,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr)]
#[repr(i8)]
pub(crate) enum TemporalityRow {
    Unspecified = 0,
    Delta = 1,
    Cumulative = 2,
}

impl From<Temporality> for TemporalityRow {
    fn from(temporality: Temporality) -> Self {
        match temporality {
            Temporality::Unspecified => Self::Unspecified,
            Temporality::Delta => Self::Delta,
            Temporality::Cumulative => Self::Cumulative,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Shape {
    pub(crate) kind: PointKindRow,
    pub(crate) temporality: Temporality,
    pub(crate) monotonic: bool,
}

#[derive(Debug, Row, Serialize)]
pub(crate) struct PointRow<'a> {
    #[serde(with = "clickhouse::serde::uuid")]
    tenant_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    project_id: Uuid,
    service_name: &'a str,
    name: &'a str,
    kind: PointKindRow,
    temporality: TemporalityRow,
    monotonic: bool,
    attributes: BTreeMap<&'a str, &'a str>,
    time: i64,
    value: f64,
    expire_at: u32,
}

impl<'a> PointRow<'a> {
    pub(crate) fn of(
        scope: Scope,
        metric: &'a Metric,
        shape: Shape,
        point: &'a MetricPoint,
        expire_at: UnixSeconds,
    ) -> Result<Self, SinkError> {
        Ok(Self {
            tenant_id: scope.tenant.as_uuid(),
            project_id: project_of(scope),
            service_name: &metric.service_name,
            name: &metric.name,
            kind: shape.kind,
            temporality: shape.temporality.into(),
            monotonic: shape.monotonic,
            attributes: borrowed(&point.attributes),
            time: nanos(point.time_unix_nanos)?,
            value: point.value,
            expire_at: seconds(expire_at)?,
        })
    }
}

#[derive(Debug, Row, Serialize)]
pub(crate) struct HistogramRow<'a> {
    #[serde(with = "clickhouse::serde::uuid")]
    tenant_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    project_id: Uuid,
    service_name: &'a str,
    name: &'a str,
    temporality: TemporalityRow,
    monotonic: bool,
    attributes: BTreeMap<&'a str, &'a str>,
    time: i64,
    count: u64,
    sum: f64,
    min: Option<f64>,
    max: Option<f64>,
    bounds: &'a [f64],
    counts: &'a [u64],
    expire_at: u32,
}

impl<'a> HistogramRow<'a> {
    pub(crate) fn of(
        scope: Scope,
        metric: &'a Metric,
        temporality: Temporality,
        point: &'a HistogramPoint,
        expire_at: UnixSeconds,
    ) -> Result<Self, SinkError> {
        Ok(Self {
            tenant_id: scope.tenant.as_uuid(),
            project_id: project_of(scope),
            service_name: &metric.service_name,
            name: &metric.name,
            temporality: temporality.into(),
            monotonic: true,
            attributes: borrowed(&point.attributes),
            time: nanos(point.time_unix_nanos)?,
            count: point.count,
            sum: point.sum,
            min: point.min,
            max: point.max,
            bounds: &point.bounds,
            counts: &point.counts,
            expire_at: seconds(expire_at)?,
        })
    }
}

#[derive(Debug, Row, Serialize)]
pub(crate) struct ExemplarRow<'a> {
    #[serde(with = "clickhouse::serde::uuid")]
    tenant_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    project_id: Uuid,
    name: &'a str,
    time: i64,
    value: f64,
    trace_id: [u8; 16],
    span_id: [u8; 8],
    attributes: BTreeMap<&'a str, &'a str>,
    expire_at: u32,
}

impl<'a> ExemplarRow<'a> {
    pub(crate) fn of(
        scope: Scope,
        metric: &'a Metric,
        exemplar: &'a Exemplar,
        expire_at: UnixSeconds,
    ) -> Result<Option<Self>, SinkError> {
        let Some(trace_id) = exemplar.trace_id else {
            return Ok(None);
        };
        Ok(Some(Self {
            tenant_id: scope.tenant.as_uuid(),
            project_id: project_of(scope),
            name: &metric.name,
            time: nanos(exemplar.time_unix_nanos)?,
            value: exemplar.value,
            trace_id: *trace_id.as_bytes(),
            span_id: exemplar
                .span_id
                .map(|id| *id.as_bytes())
                .unwrap_or_default(),
            attributes: borrowed(&exemplar.attributes),
            expire_at: seconds(expire_at)?,
        }))
    }
}

#[derive(Debug, Row, Serialize)]
pub(crate) struct CatalogueRow<'a> {
    #[serde(with = "clickhouse::serde::uuid")]
    tenant_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    project_id: Uuid,
    name: &'a str,
    kind: CatalogueKindRow,
    unit: &'a str,
    description: &'a str,
    last_seen: i64,
    expire_at: u32,
}

impl<'a> CatalogueRow<'a> {
    pub(crate) fn of(
        scope: Scope,
        metric: &'a Metric,
        expire_at: UnixSeconds,
    ) -> Result<Self, SinkError> {
        Ok(Self {
            tenant_id: scope.tenant.as_uuid(),
            project_id: project_of(scope),
            name: &metric.name,
            kind: metric.data.kind().into(),
            unit: &metric.unit,
            description: &metric.description,
            last_seen: nanos(metric.data.last_seen_unix_nanos().unwrap_or_default())?,
            expire_at: seconds(expire_at)?,
        })
    }
}

fn project_of(scope: Scope) -> Uuid {
    scope.project.map(ProjectId::as_uuid).unwrap_or_default()
}
