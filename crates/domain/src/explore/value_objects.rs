use super::errors::BadWindow;
use super::query::{Field, Query};
use super::slots::{Slot, filled};
use crate::catalogue::ProjectId;
use crate::metrics::{MIN_STEP_NANOS, STEPS};
use crate::placement::Placement;
use crate::telemetry::{SpanId, SpanKind, SpanStatus, TraceId};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanOrder {
    #[default]
    Started,
    Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanFilter {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub query: Query,
    pub order: SpanOrder,
    limit: u32,
}

impl SpanFilter {
    pub fn over(window: Window) -> Self {
        Self {
            window,
            project: None,
            query: Query::anything(),
            order: SpanOrder::default(),
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
pub struct ListedSpan {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub service: Box<str>,
    pub name: Box<str>,
    pub kind: SpanKind,
    pub status: SpanStatus,
    pub status_code: Option<u16>,
    pub started_unix_nanos: u64,
    pub duration_nanos: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Measure {
    #[default]
    Count,
    Errors,
    ErrorRate,
    Average,
    P50,
    P95,
    P99,
}

pub const MAX_GROUPS: u32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesFilter {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub query: Query,
    pub measure: Measure,
    group: Option<Field>,
}

impl SeriesFilter {
    pub fn new(
        window: Window,
        query: Query,
        measure: Measure,
        group: Option<Field>,
    ) -> Result<Self, super::errors::BadQuery> {
        if matches!(
            group,
            Some(Field::Duration | Field::TraceId | Field::SpanId)
        ) {
            return Err(super::errors::BadQuery::Shape);
        }
        Ok(Self {
            window,
            project: None,
            query,
            measure,
            group,
        })
    }

    pub fn group(&self) -> Option<&Field> {
        self.group.as_ref()
    }

    pub fn step_nanos(&self) -> u64 {
        self.span().div_ceil(STEPS).max(MIN_STEP_NANOS)
    }

    pub fn slot_starts(&self) -> Vec<u64> {
        let from = self.window.from_unix_nanos();
        let step = self.step_nanos();
        (0..self.span().div_ceil(step))
            .map(|slot| from + slot * step)
            .collect()
    }

    fn span(&self) -> u64 {
        self.window.to_unix_nanos() - self.window.from_unix_nanos()
    }
}

pub type SparseLine = (Option<Box<str>>, Vec<(u64, f64)>);

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesLine {
    pub group: Option<Box<str>>,
    pub points: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeriesResult {
    pub step_nanos: u64,
    pub starts: Vec<u64>,
    pub lines: Vec<SeriesLine>,
}

impl SeriesResult {
    pub fn dense(filter: &SeriesFilter, sparse: Vec<SparseLine>) -> Self {
        let starts = filter.slot_starts();
        let empty = match filter.measure {
            Measure::Count | Measure::Errors => Some(0.0),
            _ => None,
        };
        let lines = sparse
            .into_iter()
            .map(|(group, found)| {
                let mut points = vec![empty; starts.len()];
                for (slot, value) in found {
                    if let Some(point) =
                        usize::try_from(slot).ok().and_then(|at| points.get_mut(at))
                    {
                        *point = Some(value);
                    }
                }
                SeriesLine { group, points }
            })
            .collect();
        Self {
            step_nanos: filter.step_nanos(),
            starts,
            lines,
        }
    }
}

pub const DEFAULT_ROWS: u32 = 20;

pub const MAX_ROWS: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableFilter {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub query: Query,
    pub order: Measure,
    group: Field,
    rows: u32,
}

impl TableFilter {
    pub fn new(
        window: Window,
        query: Query,
        group: Field,
        order: Measure,
    ) -> Result<Self, super::errors::BadQuery> {
        if matches!(group, Field::Duration | Field::TraceId | Field::SpanId) {
            return Err(super::errors::BadQuery::Shape);
        }
        Ok(Self {
            window,
            project: None,
            query,
            order,
            group,
            rows: DEFAULT_ROWS,
        })
    }

    #[must_use]
    pub fn at_most(mut self, asked: u32) -> Self {
        self.rows = asked.clamp(1, MAX_ROWS);
        self
    }

    pub fn group(&self) -> &Field {
        &self.group
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub group: Box<str>,
    pub spans: u64,
    pub errors: u64,
    pub average_nanos: f64,
    pub p50_nanos: f64,
    pub p95_nanos: f64,
    pub p99_nanos: f64,
}

pub const MAX_FACETS: usize = 12;

pub const FACET_VALUES: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetFilter {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub query: Query,
    fields: Vec<Field>,
    pub duration: bool,
}

impl FacetFilter {
    pub fn new(
        window: Window,
        query: Query,
        fields: Vec<Field>,
        duration: bool,
    ) -> Result<Self, super::errors::BadQuery> {
        let countable =
            |field: &Field| !matches!(field, Field::Duration | Field::TraceId | Field::SpanId);
        if fields.len() > MAX_FACETS || !fields.iter().all(countable) {
            return Err(super::errors::BadQuery::Shape);
        }
        Ok(Self {
            window,
            project: None,
            query,
            fields,
            duration,
        })
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetValues {
    pub field: Field,
    pub values: Vec<SeenValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationRange {
    pub min_nanos: u64,
    pub max_nanos: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facets {
    pub fields: Vec<FacetValues>,
    pub duration: Option<DurationRange>,
}

pub const MAX_ATTRIBUTES: u32 = 200;

pub const MAX_VALUES: u32 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeFilter {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub prefix: Option<Box<str>>,
}

impl AttributeFilter {
    pub fn over(window: Window) -> Self {
        Self {
            window,
            project: None,
            prefix: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenAttribute {
    pub key: Box<str>,
    pub spans: u64,
    pub numeric: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenValue {
    pub value: Box<str>,
    pub spans: u64,
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
pub struct ServiceEdge {
    pub caller: Box<str>,
    pub callee: Box<str>,
    pub calls: u64,
    pub errors: u64,
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

impl Slot for ServicePoint {
    fn idle(start_unix_nanos: u64) -> Self {
        Self {
            start_unix_nanos,
            requests: 0,
            errors: 0,
            p95_nanos: None,
        }
    }

    fn start_unix_nanos(self) -> u64 {
        self.start_unix_nanos
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
        self.step_nanos = filter.step_nanos();
        self.points = filled(core::mem::take(&mut self.points), filter);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheGrouping {
    #[default]
    Operation,
    Store,
    KeyPattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheSummary {
    pub operation: Box<str>,
    pub store: Box<str>,
    pub calls: u64,
    pub hits: u64,
    pub misses: u64,
    pub p50_nanos: u64,
    pub p95_nanos: u64,
    pub last_seen_unix_nanos: u64,
}

impl CacheSummary {
    pub fn hit_rate(&self) -> Option<f64> {
        hit_rate(self.hits, self.misses)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePoint {
    pub start_unix_nanos: u64,
    pub calls: u64,
    pub hits: u64,
    pub misses: u64,
    pub p95_nanos: Option<u64>,
}

impl Slot for CachePoint {
    fn idle(start_unix_nanos: u64) -> Self {
        Self {
            start_unix_nanos,
            calls: 0,
            hits: 0,
            misses: 0,
            p95_nanos: None,
        }
    }

    fn start_unix_nanos(self) -> u64 {
        self.start_unix_nanos
    }
}

impl CachePoint {
    pub fn hit_rate(&self) -> Option<f64> {
        hit_rate(self.hits, self.misses)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheSeries {
    pub step_nanos: u64,
    pub calls: u64,
    pub hits: u64,
    pub misses: u64,
    pub p50_nanos: Option<u64>,
    pub p95_nanos: Option<u64>,
    pub p99_nanos: Option<u64>,
    pub points: Vec<CachePoint>,
}

impl CacheSeries {
    pub fn hit_rate(&self) -> Option<f64> {
        hit_rate(self.hits, self.misses)
    }

    #[must_use]
    pub fn filled(mut self, filter: &ServiceFilter) -> Self {
        self.step_nanos = filter.step_nanos();
        self.points = filled(core::mem::take(&mut self.points), filter);
        self
    }
}

#[allow(clippy::cast_precision_loss)]
fn hit_rate(hits: u64, misses: u64) -> Option<f64> {
    let asked = hits.checked_add(misses).filter(|total| *total > 0)?;
    Some(hits as f64 / asked as f64)
}
