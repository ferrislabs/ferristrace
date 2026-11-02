use super::errors::BadLogQuery;
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::placement::Placement;
use crate::telemetry::{LogRecord, Severity, TraceId};

pub const DEFAULT_LOG_LIMIT: u32 = 100;
pub const MAX_LOG_LIMIT: u32 = 500;
pub const DEFAULT_BUCKETS: u32 = 60;
pub const MAX_BUCKETS: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogCursor {
    pub time_unix_nanos: u64,
    pub tiebreak: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogQuery {
    pub window: Window,
    pub services: Vec<Box<str>>,
    pub severities: Vec<Severity>,
    pub text: Option<Box<str>>,
    pub attributes: Vec<(Box<str>, Box<str>)>,
    pub trace: Option<TraceId>,
    pub project: Option<ProjectId>,
    pub before: Option<LogCursor>,
    pub placement: Placement,
    limit: u32,
}

impl LogQuery {
    pub fn over(window: Window) -> Self {
        Self {
            window,
            services: Vec::new(),
            severities: Vec::new(),
            text: None,
            attributes: Vec::new(),
            trace: None,
            project: None,
            before: None,
            placement: Placement::default(),
            limit: DEFAULT_LOG_LIMIT,
        }
    }

    pub fn at_most(mut self, asked: u32) -> Result<Self, BadLogQuery> {
        if !(1..=MAX_LOG_LIMIT).contains(&asked) {
            return Err(BadLogQuery::Limit { max: MAX_LOG_LIMIT });
        }
        self.limit = asked;
        Ok(self)
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogPage {
    pub records: Vec<LogRecord>,
    pub next: Option<LogCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Buckets(u32);

impl Default for Buckets {
    fn default() -> Self {
        Self(DEFAULT_BUCKETS)
    }
}

impl TryFrom<u32> for Buckets {
    type Error = BadLogQuery;

    fn try_from(asked: u32) -> Result<Self, BadLogQuery> {
        if (1..=MAX_BUCKETS).contains(&asked) {
            Ok(Self(asked))
        } else {
            Err(BadLogQuery::Buckets { max: MAX_BUCKETS })
        }
    }
}

impl Buckets {
    pub fn count(self) -> u32 {
        self.0
    }

    pub fn width_over(self, window: Window) -> u64 {
        span_of(window).div_ceil(u64::from(self.0)).max(1)
    }

    pub fn laid_over(self, window: Window) -> Vec<LogBucket> {
        let width = self.width_over(window);
        let slots = span_of(window).div_ceil(width);
        (0..slots)
            .map(|slot| LogBucket::empty(window.from_unix_nanos() + slot * width))
            .collect()
    }
}

fn span_of(window: Window) -> u64 {
    window.to_unix_nanos() - window.from_unix_nanos()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogBucket {
    pub start_unix_nanos: u64,
    counts: [u64; Severity::ALL.len()],
}

impl LogBucket {
    pub fn empty(start_unix_nanos: u64) -> Self {
        Self {
            start_unix_nanos,
            counts: [0; Severity::ALL.len()],
        }
    }

    pub fn add(&mut self, severity: Severity, count: u64) {
        let held = &mut self.counts[severity.rank()];
        *held = held.saturating_add(count);
    }

    pub fn count(&self, severity: Severity) -> u64 {
        self.counts[severity.rank()]
    }

    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }
}
