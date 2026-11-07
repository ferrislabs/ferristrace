use super::slots::{Slot, filled};
use super::value_objects::ServiceFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatementGrouping {
    #[default]
    Statement,
    Database,
    Operation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementSummary {
    pub statement: Box<str>,
    pub database: Box<str>,
    pub system: Box<str>,
    pub calls: u64,
    pub errors: u64,
    pub p50_nanos: u64,
    pub p95_nanos: u64,
    pub total_nanos: u64,
    pub last_seen_unix_nanos: u64,
}

impl StatementSummary {
    pub fn error_rate(&self) -> Option<f64> {
        rate(self.errors, self.calls)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatementPoint {
    pub start_unix_nanos: u64,
    pub calls: u64,
    pub errors: u64,
    pub p95_nanos: Option<u64>,
}

impl Slot for StatementPoint {
    fn idle(start_unix_nanos: u64) -> Self {
        Self {
            start_unix_nanos,
            calls: 0,
            errors: 0,
            p95_nanos: None,
        }
    }

    fn start_unix_nanos(self) -> u64 {
        self.start_unix_nanos
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatementSeries {
    pub step_nanos: u64,
    pub calls: u64,
    pub errors: u64,
    pub p50_nanos: Option<u64>,
    pub p95_nanos: Option<u64>,
    pub p99_nanos: Option<u64>,
    pub points: Vec<StatementPoint>,
}

impl StatementSeries {
    pub fn error_rate(&self) -> Option<f64> {
        rate(self.errors, self.calls)
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

pub const LEAST_FANOUT: u64 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fanout {
    pub statement: Box<str>,
    pub traces: u64,
    pub worst: u64,
    pub calls: u64,
}
