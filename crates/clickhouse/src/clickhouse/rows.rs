use std::collections::BTreeMap;

use clickhouse::Row;
use ferristrace_domain::{
    EXCEPTION_EVENT, Fingerprint, PRINT_BYTES, ProjectId, Scope, SinkError, Span, SpanEvent,
    SpanKind, SpanStatus, UnixSeconds,
};
use serde::Serialize;
use serde_repr::Serialize_repr;
use uuid::Uuid;

const HTTP_STATUS_CODE: &str = "http.status_code";

pub(crate) const EXCEPTION_TYPE: &str = "exception.type";
pub(crate) const EXCEPTION_MESSAGE: &str = "exception.message";
pub(crate) const EXCEPTION_STACKTRACE: &str = "exception.stacktrace";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr)]
#[repr(i8)]
pub(crate) enum KindRow {
    Unspecified = 0,
    Internal = 1,
    Server = 2,
    Client = 3,
    Producer = 4,
    Consumer = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr)]
#[repr(i8)]
pub(crate) enum StatusRow {
    Unset = 0,
    Ok = 1,
    Error = 2,
}

impl From<SpanKind> for KindRow {
    fn from(kind: SpanKind) -> Self {
        match kind {
            SpanKind::Unspecified => Self::Unspecified,
            SpanKind::Internal => Self::Internal,
            SpanKind::Server => Self::Server,
            SpanKind::Client => Self::Client,
            SpanKind::Producer => Self::Producer,
            SpanKind::Consumer => Self::Consumer,
        }
    }
}

impl From<SpanStatus> for StatusRow {
    fn from(status: SpanStatus) -> Self {
        match status {
            SpanStatus::Unset => Self::Unset,
            SpanStatus::Ok => Self::Ok,
            SpanStatus::Error => Self::Error,
        }
    }
}

#[derive(Debug, Row, Serialize)]
pub(crate) struct SpanRow<'a> {
    #[serde(with = "clickhouse::serde::uuid")]
    pub(crate) tenant_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    pub(crate) project_id: Uuid,
    pub(crate) service_name: &'a str,
    pub(crate) start_time: i64,
    pub(crate) end_time: i64,
    pub(crate) trace_id: [u8; 16],
    pub(crate) span_id: [u8; 8],
    pub(crate) parent_span_id: Option<[u8; 8]>,
    pub(crate) name: &'a str,
    pub(crate) kind: KindRow,
    pub(crate) status: StatusRow,
    pub(crate) http_status_code: Option<u16>,
    pub(crate) attributes: BTreeMap<&'a str, &'a str>,
    pub(crate) expire_at: u32,
}

impl<'a> SpanRow<'a> {
    pub(crate) fn of(
        scope: Scope,
        span: &'a Span,
        expire_at: UnixSeconds,
    ) -> Result<Self, SinkError> {
        Ok(Self {
            tenant_id: scope.tenant.as_uuid(),
            project_id: scope.project.map(ProjectId::as_uuid).unwrap_or_default(),
            service_name: &span.service_name,
            start_time: nanos(span.start_unix_nanos)?,
            end_time: nanos(span.end_unix_nanos)?,
            trace_id: *span.trace_id.as_bytes(),
            span_id: *span.span_id.as_bytes(),
            parent_span_id: span.parent.map(|id| *id.as_bytes()),
            name: &span.name,
            kind: span.kind.into(),
            status: span.status.into(),
            http_status_code: span
                .attributes
                .get(HTTP_STATUS_CODE)
                .and_then(|value| value.parse().ok()),
            attributes: span
                .attributes
                .iter()
                .map(|(key, value)| (&**key, &**value))
                .collect(),
            expire_at: seconds(expire_at)?,
        })
    }
}

#[derive(Debug, Row, Serialize)]
pub(crate) struct EventRow<'a> {
    #[serde(with = "clickhouse::serde::uuid")]
    pub(crate) tenant_id: Uuid,
    #[serde(with = "clickhouse::serde::uuid")]
    pub(crate) project_id: Uuid,
    pub(crate) service_name: &'a str,
    pub(crate) time: i64,
    pub(crate) trace_id: [u8; 16],
    pub(crate) span_id: [u8; 8],
    pub(crate) name: &'a str,
    pub(crate) fingerprint: [u8; PRINT_BYTES],
    pub(crate) attributes: BTreeMap<&'a str, &'a str>,
    pub(crate) expire_at: u32,
}

impl<'a> EventRow<'a> {
    pub(crate) fn of(
        scope: Scope,
        span: &'a Span,
        event: &'a SpanEvent,
        expire_at: UnixSeconds,
    ) -> Result<Self, SinkError> {
        Ok(Self {
            tenant_id: scope.tenant.as_uuid(),
            project_id: scope.project.map(ProjectId::as_uuid).unwrap_or_default(),
            service_name: &span.service_name,
            time: nanos(event.time_unix_nanos)?,
            trace_id: *span.trace_id.as_bytes(),
            span_id: *span.span_id.as_bytes(),
            name: &event.name,
            fingerprint: print_of(span, event),
            attributes: event
                .attributes
                .iter()
                .map(|(key, value)| (&**key, &**value))
                .collect(),
            expire_at: seconds(expire_at)?,
        })
    }
}

fn print_of(span: &Span, event: &SpanEvent) -> [u8; PRINT_BYTES] {
    if &*event.name != EXCEPTION_EVENT {
        return [0; PRINT_BYTES];
    }
    *Fingerprint::of(
        &span.service_name,
        said(event, EXCEPTION_TYPE),
        said(event, EXCEPTION_MESSAGE),
    )
    .as_bytes()
}

pub(crate) fn said<'a>(event: &'a SpanEvent, key: &str) -> &'a str {
    event
        .attributes
        .get(key)
        .map(|value| &**value)
        .unwrap_or_default()
}

fn nanos(value: u64) -> Result<i64, SinkError> {
    i64::try_from(value).map_err(|_| SinkError::TimestampOutOfRange)
}

fn seconds(value: UnixSeconds) -> Result<u32, SinkError> {
    u32::try_from(value.get()).map_err(|_| SinkError::TimestampOutOfRange)
}
