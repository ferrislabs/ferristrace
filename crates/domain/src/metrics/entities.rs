use std::collections::BTreeMap;

use super::value_objects::{MetricKind, Temporality};
use crate::telemetry::{SpanId, TraceId};

#[derive(Debug, Clone, PartialEq)]
pub struct Exemplar {
    pub time_unix_nanos: u64,
    pub value: f64,
    pub trace_id: Option<TraceId>,
    pub span_id: Option<SpanId>,
    pub attributes: BTreeMap<Box<str>, Box<str>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricPoint {
    pub time_unix_nanos: u64,
    pub value: f64,
    pub attributes: BTreeMap<Box<str>, Box<str>>,
    pub exemplars: Vec<Exemplar>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistogramPoint {
    pub time_unix_nanos: u64,
    pub count: u64,
    pub sum: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub bounds: Vec<f64>,
    pub counts: Vec<u64>,
    pub attributes: BTreeMap<Box<str>, Box<str>>,
    pub exemplars: Vec<Exemplar>,
}

impl HistogramPoint {
    #[allow(clippy::cast_precision_loss)]
    pub fn quantile(&self, q: f64) -> Option<f64> {
        if self.counts.len() != self.bounds.len() + 1 {
            return None;
        }
        let total: u64 = self.counts.iter().sum();
        if total == 0 {
            return None;
        }
        let rank = q.clamp(0.0, 1.0) * total as f64;
        let mut before = 0u64;
        for (index, &count) in self.counts.iter().enumerate() {
            let through = before + count;
            if count > 0 && through as f64 >= rank {
                let (lower, upper) = self.edges(index);
                let share = (rank - before as f64) / count as f64;
                return Some(lower + (upper - lower) * share.clamp(0.0, 1.0));
            }
            before = through;
        }
        None
    }

    fn edges(&self, index: usize) -> (f64, f64) {
        let last = self.bounds.len();
        let lower = if index == 0 {
            self.min
                .unwrap_or_else(|| self.bounds.first().map_or(0.0, |first| first.min(0.0)))
        } else {
            self.bounds[index - 1]
        };
        let upper = if index == last {
            self.max
                .unwrap_or_else(|| self.bounds.last().copied().unwrap_or(lower))
        } else {
            self.bounds[index]
        };
        let lower = self.min.map_or(lower, |min| lower.max(min));
        let upper = self.max.map_or(upper, |max| upper.min(max));
        (lower, upper.max(lower))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetricData {
    Gauge(Vec<MetricPoint>),
    Sum {
        monotonic: bool,
        temporality: Temporality,
        points: Vec<MetricPoint>,
    },
    Histogram {
        temporality: Temporality,
        points: Vec<HistogramPoint>,
    },
}

impl MetricData {
    pub fn kind(&self) -> MetricKind {
        match self {
            Self::Gauge(_) => MetricKind::Gauge,
            Self::Sum { .. } => MetricKind::Sum,
            Self::Histogram { .. } => MetricKind::Histogram,
        }
    }

    pub fn last_seen_unix_nanos(&self) -> Option<u64> {
        match self {
            Self::Gauge(points) | Self::Sum { points, .. } => {
                points.iter().map(|point| point.time_unix_nanos).max()
            }
            Self::Histogram { points, .. } => {
                points.iter().map(|point| point.time_unix_nanos).max()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    pub name: Box<str>,
    pub unit: Box<str>,
    pub description: Box<str>,
    pub service_name: Box<str>,
    pub data: MetricData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricDescriptor {
    pub name: Box<str>,
    pub kind: MetricKind,
    pub unit: Box<str>,
    pub description: Box<str>,
    pub last_seen_unix_nanos: u64,
}
