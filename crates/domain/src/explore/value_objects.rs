use super::errors::BadWindow;
use super::query::Query;
use crate::catalogue::ProjectId;
use crate::metrics::{MIN_STEP_NANOS, STEPS};
use crate::placement::Placement;
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
    pub placement: Placement,
    pub query: Query,
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
            placement: Placement::default(),
            query: Query::default(),
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
    pub status_code: Option<u16>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceFilter {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub environment: Option<Box<str>>,
    pub placement: Placement,
}

impl ServiceFilter {
    pub fn over(window: Window) -> Self {
        Self {
            window,
            project: None,
            environment: None,
            placement: Placement::default(),
        }
    }

    pub fn step_nanos(&self) -> u64 {
        self.span().div_ceil(STEPS).max(MIN_STEP_NANOS)
    }

    pub fn slot_starts(&self) -> impl Iterator<Item = u64> {
        let from = self.window.from_unix_nanos();
        let step = self.step_nanos();
        (0..self.span().div_ceil(step)).map(move |slot| from + slot * step)
    }

    fn span(&self) -> u64 {
        self.window.to_unix_nanos() - self.window.from_unix_nanos()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServicePoint {
    pub start_unix_nanos: u64,
    pub requests: u64,
    pub errors: u64,
    pub p95_nanos: Option<u64>,
}

impl ServicePoint {
    pub fn idle(start_unix_nanos: u64) -> Self {
        Self {
            start_unix_nanos,
            requests: 0,
            errors: 0,
            p95_nanos: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServiceSeries {
    pub step_nanos: u64,
    pub requests: u64,
    pub errors: u64,
    pub p50_nanos: Option<u64>,
    pub p95_nanos: Option<u64>,
    pub p99_nanos: Option<u64>,
    pub points: Vec<ServicePoint>,
}

impl ServiceSeries {
    #[must_use]
    pub fn filled(mut self, filter: &ServiceFilter) -> Self {
        let mut served = core::mem::take(&mut self.points).into_iter().peekable();
        self.step_nanos = filter.step_nanos();
        self.points = filter
            .slot_starts()
            .map(|start| {
                while served
                    .peek()
                    .is_some_and(|point| point.start_unix_nanos < start)
                {
                    served.next();
                }
                served
                    .next_if(|point| point.start_unix_nanos == start)
                    .unwrap_or_else(|| ServicePoint::idle(start))
            })
            .collect();
        self
    }
}
