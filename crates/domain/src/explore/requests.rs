use super::slots::{Slot, filled};
use super::value_objects::ServiceFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestGrouping {
    #[default]
    Route,
    Service,
    Method,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSummary {
    pub route: Box<str>,
    pub service: Box<str>,
    pub method: Box<str>,
    pub requests: u64,
    pub answered: u64,
    pub refused: u64,
    pub broken: u64,
    pub p50_nanos: u64,
    pub p95_nanos: u64,
    pub total_nanos: u64,
    pub last_seen_unix_nanos: u64,
}

impl RequestSummary {
    pub fn broken_rate(&self) -> Option<f64> {
        rate(self.broken, self.requests)
    }

    pub fn refused_rate(&self) -> Option<f64> {
        rate(self.refused, self.requests)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestPoint {
    pub start_unix_nanos: u64,
    pub requests: u64,
    pub refused: u64,
    pub broken: u64,
    pub p95_nanos: Option<u64>,
}

impl Slot for RequestPoint {
    fn idle(start_unix_nanos: u64) -> Self {
        Self {
            start_unix_nanos,
            requests: 0,
            refused: 0,
            broken: 0,
            p95_nanos: None,
        }
    }

    fn start_unix_nanos(self) -> u64 {
        self.start_unix_nanos
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestSeries {
    pub step_nanos: u64,
    pub requests: u64,
    pub answered: u64,
    pub refused: u64,
    pub broken: u64,
    pub p50_nanos: Option<u64>,
    pub p95_nanos: Option<u64>,
    pub p99_nanos: Option<u64>,
    pub points: Vec<RequestPoint>,
}

impl RequestSeries {
    pub fn broken_rate(&self) -> Option<f64> {
        rate(self.broken, self.requests)
    }

    pub fn refused_rate(&self) -> Option<f64> {
        rate(self.refused, self.requests)
    }

    #[must_use]
    pub fn filled(mut self, filter: &ServiceFilter) -> Self {
        self.step_nanos = filter.step_nanos();
        self.points = filled(core::mem::take(&mut self.points), filter);
        self
    }
}

#[allow(clippy::cast_precision_loss)]
fn rate(part: u64, whole: u64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}
