use std::collections::BTreeMap;

use super::value_objects::{Severity, SpanEvent, SpanId, SpanKind, SpanStatus, TraceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent: Option<SpanId>,
    pub name: Box<str>,
    pub service_name: Box<str>,
    pub kind: SpanKind,
    pub status: SpanStatus,
    pub start_unix_nanos: u64,
    pub end_unix_nanos: u64,
    pub attributes: BTreeMap<Box<str>, Box<str>>,
    pub events: Vec<SpanEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    pub time_unix_nanos: u64,
    pub observed_unix_nanos: u64,
    pub severity: Severity,
    pub severity_number: u8,
    pub service_name: Box<str>,
    pub body: Box<str>,
    pub attributes: BTreeMap<Box<str>, Box<str>>,
    pub resource: BTreeMap<Box<str>, Box<str>>,
    pub trace_id: Option<TraceId>,
    pub span_id: Option<SpanId>,
}
