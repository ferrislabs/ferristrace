use super::errors::BadWindow;
use crate::catalogue::ProjectId;
use crate::telemetry::TraceId;

const DEFAULT_LIMIT: u32 = 50;

const MAX_LIMIT: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    from_unix_nanos: u64,
    to_unix_nanos: u64,
}

impl Window {
    pub fn new(from_unix_nanos: u64, to_unix_nanos: u64) -> Result<Self, BadWindow> {
        if to_unix_nanos <= from_unix_nanos {
            return Err(BadWindow::Backwards);
        }
        Ok(Self {
            from_unix_nanos,
            to_unix_nanos,
        })
    }

    pub fn from_unix_nanos(self) -> u64 {
        self.from_unix_nanos
    }

    pub fn to_unix_nanos(self) -> u64 {
        self.to_unix_nanos
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraceOrder {
    #[default]
    Started,
    Duration,
    Spans,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceFilter {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub service: Option<Box<str>>,
    pub name: Option<Box<str>>,
    pub only_errors: bool,
    pub at_least_nanos: Option<u64>,
    pub order: TraceOrder,
    limit: u32,
}

impl TraceFilter {
    pub fn over(window: Window) -> Self {
        Self {
            window,
            project: None,
            service: None,
            name: None,
            only_errors: false,
            at_least_nanos: None,
            order: TraceOrder::default(),
            limit: DEFAULT_LIMIT,
        }
    }

    #[must_use]
    pub fn at_most(mut self, asked: u32) -> Self {
        self.limit = asked.clamp(1, MAX_LIMIT);
        self
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSummary {
    pub trace_id: TraceId,
    pub root_service: Box<str>,
    pub root_name: Box<str>,
    pub started_unix_nanos: u64,
    pub duration_nanos: u64,
    pub spans: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSummary {
    pub name: Box<str>,
    pub requests: u64,
    pub errors: u64,
    pub p50_nanos: u64,
    pub p95_nanos: u64,
    pub p99_nanos: u64,
    pub last_seen_unix_nanos: u64,
}
