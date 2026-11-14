use core::fmt;
use std::collections::BTreeMap;

use super::entities::{Exemplar, Metric};
use super::errors::{BadAggregation, BadMetricKind, BadMetricQuery};
use crate::catalogue::ProjectId;
use crate::explore::{Query, Stream, Window};
use crate::placement::Placement;
use crate::telemetry::DecompressedBytes;

pub const STEPS: u64 = 60;
pub const MIN_STEP_NANOS: u64 = 1_000_000_000;
pub const MAX_GROUP_BY: usize = 8;
pub const MAX_EXEMPLARS: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MetricKind {
    Gauge,
    Sum,
    Histogram,
}

impl MetricKind {
    pub const ALL: [Self; 3] = [Self::Gauge, Self::Sum, Self::Histogram];

    pub fn name(self) -> &'static str {
        match self {
            Self::Gauge => "gauge",
            Self::Sum => "sum",
            Self::Histogram => "histogram",
        }
    }
}

impl TryFrom<&str> for MetricKind {
    type Error = BadMetricKind;

    fn try_from(spelling: &str) -> Result<Self, BadMetricKind> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == spelling)
            .ok_or(BadMetricKind)
    }
}

impl fmt::Display for MetricKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Temporality {
    Unspecified,
    Delta,
    Cumulative,
}

impl Temporality {
    pub fn name(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Delta => "delta",
            Self::Cumulative => "cumulative",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Aggregation {
    Sum,
    Avg,
    Min,
    Max,
    Rate,
    P50,
    P95,
    P99,
}

impl Aggregation {
    pub const ALL: [Self; 8] = [
        Self::Sum,
        Self::Avg,
        Self::Min,
        Self::Max,
        Self::Rate,
        Self::P50,
        Self::P95,
        Self::P99,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
            Self::Rate => "rate",
            Self::P50 => "p50",
            Self::P95 => "p95",
            Self::P99 => "p99",
        }
    }

    pub fn quantile(self) -> Option<f64> {
        match self {
            Self::P50 => Some(0.5),
            Self::P95 => Some(0.95),
            Self::P99 => Some(0.99),
            Self::Sum | Self::Avg | Self::Min | Self::Max | Self::Rate => None,
        }
    }

    pub fn applies_to(self, kind: MetricKind) -> bool {
        match kind {
            MetricKind::Gauge => matches!(self, Self::Sum | Self::Avg | Self::Min | Self::Max),
            MetricKind::Sum => self.quantile().is_none(),
            MetricKind::Histogram => {
                matches!(self, Self::Avg | Self::Rate) || self.quantile().is_some()
            }
        }
    }

    pub fn default_for(kind: MetricKind) -> Self {
        match kind {
            MetricKind::Gauge => Self::Avg,
            MetricKind::Sum => Self::Rate,
            MetricKind::Histogram => Self::P95,
        }
    }
}

impl TryFrom<&str> for Aggregation {
    type Error = BadAggregation;

    fn try_from(spelling: &str) -> Result<Self, BadAggregation> {
        Self::ALL
            .into_iter()
            .find(|aggregation| aggregation.name() == spelling)
            .ok_or(BadAggregation)
    }
}

impl fmt::Display for Aggregation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricPayload {
    pub metrics: Vec<Metric>,
    pub decompressed: DecompressedBytes,
}

impl MetricPayload {
    pub fn datapoints(&self) -> u64 {
        self.metrics
            .iter()
            .map(|metric| metric.data.datapoints() as u64)
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricQuery {
    pub window: Window,
    pub name: Box<str>,
    pub group_by: Vec<Box<str>>,
    pub aggregation: Option<Aggregation>,
    pub project: Option<ProjectId>,
    pub placement: Placement,
    pub filter: Query,
}

impl MetricQuery {
    pub fn over(window: Window, name: &str) -> Self {
        Self {
            window,
            name: name.into(),
            group_by: Vec::new(),
            aggregation: None,
            project: None,
            placement: Placement::default(),
            filter: Query::default(),
        }
    }

    pub fn resolved(&self, kind: MetricKind) -> Result<Aggregation, BadMetricQuery> {
        self.filter
            .fits(Stream::Metrics)
            .map_err(|_| BadMetricQuery::Field)?;
        if self.group_by.len() > MAX_GROUP_BY {
            return Err(BadMetricQuery::GroupBy { max: MAX_GROUP_BY });
        }
        let aggregation = self
            .aggregation
            .unwrap_or_else(|| Aggregation::default_for(kind));
        if aggregation.applies_to(kind) {
            Ok(aggregation)
        } else {
            Err(BadMetricQuery::Inapplicable { aggregation, kind })
        }
    }

    pub fn step_nanos(&self) -> u64 {
        self.span().div_ceil(STEPS).max(MIN_STEP_NANOS)
    }

    pub fn slots(&self) -> u64 {
        self.span().div_ceil(self.step_nanos())
    }

    pub fn slot_starts(&self) -> impl Iterator<Item = u64> {
        let from = self.window.from_unix_nanos();
        let step = self.step_nanos();
        (0..self.slots()).map(move |slot| from + slot * step)
    }

    fn span(&self) -> u64 {
        self.window.to_unix_nanos() - self.window.from_unix_nanos()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Series {
    pub labels: BTreeMap<Box<str>, Box<str>>,
    pub points: Vec<(u64, Option<f64>)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeriesSet {
    pub step_nanos: u64,
    pub series: Vec<Series>,
    pub exemplars: Vec<Exemplar>,
}
