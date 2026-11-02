use std::collections::BTreeMap;

use super::value_objects::{SpanEvent, SpanId, SpanKind, SpanStatus, TraceId};

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
