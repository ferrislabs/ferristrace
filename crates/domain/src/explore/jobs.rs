use super::slots::{Slot, filled};
use super::value_objects::ServiceFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JobGrouping {
    #[default]
    Job,
    Queue,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSummary {
    pub job: Box<str>,
    pub queue: Box<str>,
    pub system: Box<str>,
    pub processed: u64,
    pub failed: u64,
    pub p50_nanos: u64,
    pub p95_nanos: u64,
    pub total_nanos: u64,
    pub last_seen_unix_nanos: u64,
}

impl JobSummary {
    pub fn failure_rate(&self) -> Option<f64> {
        rate(self.failed, self.processed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobPoint {
    pub start_unix_nanos: u64,
    pub processed: u64,
    pub failed: u64,
    pub p95_nanos: Option<u64>,
}

impl Slot for JobPoint {
    fn idle(start_unix_nanos: u64) -> Self {
        Self {
            start_unix_nanos,
            processed: 0,
            failed: 0,
            p95_nanos: None,
        }
    }

    fn start_unix_nanos(self) -> u64 {
        self.start_unix_nanos
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JobSeries {
    pub step_nanos: u64,
    pub processed: u64,
    pub failed: u64,
    pub p50_nanos: Option<u64>,
    pub p95_nanos: Option<u64>,
    pub p99_nanos: Option<u64>,
    pub points: Vec<JobPoint>,
}

impl JobSeries {
    pub fn failure_rate(&self) -> Option<f64> {
        rate(self.failed, self.processed)
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
