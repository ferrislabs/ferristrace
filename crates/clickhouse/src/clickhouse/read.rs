use std::collections::{BTreeMap, HashMap};

use clickhouse::{Client, Row};
use ferristrace_domain::{
    ProjectId, ServiceSummary, Span, SpanEvent, SpanId, SpanKind, SpanReader, SpanStatus,
    StoreError, TenantId, TraceFilter, TraceId, TraceOrder, TraceSummary, Window,
};
use serde::Deserialize;
use uuid::Uuid;

const SERVICES: &str = "SELECT \
     service_name AS name, \
     count() AS requests, \
     countIf(status = 'error') AS errors, \
     quantiles(0.5, 0.95, 0.99)( \
       toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time) \
     ) AS latency, \
     toUnixTimestamp64Nano(max(start_time)) AS last_seen \
   FROM spans FINAL \
   WHERE tenant_id = ? \
     AND start_time >= fromUnixTimestamp64Nano(?) \
     AND start_time < fromUnixTimestamp64Nano(?) \
     AND kind IN ('server', 'consumer') \
     AND (? = 0 OR project_id = ?) \
   GROUP BY service_name \
   ORDER BY requests DESC \
   LIMIT 500";

const SUMMARIES: &str = "SELECT \
     lower(hex(trace_id)) AS trace_id, \
     argMin(service_name, (parent_span_id IS NOT NULL, start_time)) AS root_service, \
     argMin(name, (parent_span_id IS NOT NULL, start_time)) AS root_name, \
     toUnixTimestamp64Nano(min(start_time)) AS started_at, \
     toUnixTimestamp64Nano(max(end_time)) - toUnixTimestamp64Nano(min(start_time)) AS duration, \
     count() AS spans, \
     countIf(status = 'error') AS errors \
   FROM spans FINAL \
   WHERE tenant_id = ? \
     AND start_time >= fromUnixTimestamp64Nano(?) \
     AND start_time < fromUnixTimestamp64Nano(?) \
     AND (? = 0 OR project_id = ?) \
   GROUP BY trace_id \
   HAVING (? = '' OR countIf(service_name = ?) > 0) \
     AND (? = '' OR countIf(positionCaseInsensitive(name, ?) > 0) > 0) \
     AND (? = 0 OR errors > 0) \
     AND duration >= ? \
   ORDER BY {order} DESC \
   LIMIT ?";

const ONE_TRACE: &str = "SELECT \
     lower(hex(span_id)) AS span_id, \
     lower(hex(parent_span_id)) AS parent_span_id, \
     service_name AS service_name, \
     name AS name, \
     toString(kind) AS kind, \
     toString(status) AS status, \
     toUnixTimestamp64Nano(start_time) AS start_time, \
     toUnixTimestamp64Nano(end_time) AS end_time, \
     attributes AS attributes \
   FROM spans FINAL \
   WHERE tenant_id = ? AND trace_id = unhex(?) \
     AND (? = 0 OR project_id = ?) \
   ORDER BY start_time, span_id";

const EVENTS_OF_TRACE: &str = "SELECT \
     lower(hex(span_id)) AS span_id, \
     name AS name, \
     toUnixTimestamp64Nano(time) AS time, \
     attributes AS attributes \
   FROM span_events FINAL \
   WHERE tenant_id = ? AND trace_id = unhex(?) \
     AND (? = 0 OR project_id = ?) \
   ORDER BY time, name \
   LIMIT 10000";

#[derive(Debug, Clone)]
pub struct ClickhouseReader {
    client: Client,
}

#[derive(Debug, Row, Deserialize)]
struct ServiceRow {
    name: String,
    requests: u64,
    errors: u64,
    latency: Vec<f64>,
    last_seen: i64,
}

#[derive(Debug, Row, Deserialize)]
struct SummaryRow {
    trace_id: String,
    root_service: String,
    root_name: String,
    started_at: i64,
    duration: i64,
    spans: u64,
    errors: u64,
}

#[derive(Debug, Row, Deserialize)]
struct EventRow {
    span_id: String,
    name: String,
    time: i64,
    attributes: Vec<(String, String)>,
}

#[derive(Debug, Row, Deserialize)]
struct SpanRow {
    span_id: String,
    parent_span_id: Option<String>,
    service_name: String,
    name: String,
    kind: String,
    status: String,
    start_time: i64,
    end_time: i64,
    attributes: Vec<(String, String)>,
}

impl ClickhouseReader {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl SpanReader for ClickhouseReader {
    async fn services(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        let rows: Vec<ServiceRow> = self
            .client
            .query(SERVICES)
            .bind(tenant.as_uuid())
            .bind(nanos(window.from_unix_nanos()))
            .bind(nanos(window.to_unix_nanos()))
            .bind(u8::from(project.is_some()))
            .bind(uuid_of(project))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        Ok(rows.into_iter().map(read_service).collect())
    }

    async fn traces(
        &self,
        tenant: TenantId,
        filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        let service = filter.service.as_deref().unwrap_or_default();
        let name = filter.name.as_deref().unwrap_or_default();
        let rows: Vec<SummaryRow> = self
            .client
            .query(&SUMMARIES.replace("{order}", ordering(filter.order)))
            .bind(tenant.as_uuid())
            .bind(nanos(filter.window.from_unix_nanos()))
            .bind(nanos(filter.window.to_unix_nanos()))
            .bind(u8::from(filter.project.is_some()))
            .bind(uuid_of(filter.project))
            .bind(service)
            .bind(service)
            .bind(name)
            .bind(name)
            .bind(u8::from(filter.only_errors))
            .bind(nanos(filter.at_least_nanos.unwrap_or_default()))
            .bind(filter.limit())
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        rows.into_iter().map(read_summary).collect()
    }

    async fn trace(
        &self,
        tenant: TenantId,
        id: TraceId,
        project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        let rows: Vec<SpanRow> = self
            .client
            .query(ONE_TRACE)
            .bind(tenant.as_uuid())
            .bind(hex::encode(id.as_bytes()))
            .bind(u8::from(project.is_some()))
            .bind(uuid_of(project))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let events: Vec<EventRow> = self
            .client
            .query(EVENTS_OF_TRACE)
            .bind(tenant.as_uuid())
            .bind(hex::encode(id.as_bytes()))
            .bind(u8::from(project.is_some()))
            .bind(uuid_of(project))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let mut by_span: HashMap<String, Vec<SpanEvent>> = HashMap::new();
        for row in events {
            by_span.entry(row.span_id).or_default().push(SpanEvent {
                time_unix_nanos: unsigned(row.time),
                name: row.name.into(),
                attributes: map_of(row.attributes),
            });
        }

        rows.into_iter()
            .map(|row| {
                let events = by_span.remove(&row.span_id).unwrap_or_default();
                read_span(id, row, events)
            })
            .collect()
    }
}

fn read_service(row: ServiceRow) -> ServiceSummary {
    ServiceSummary {
        name: row.name.into(),
        requests: row.requests,
        errors: row.errors,
        p50_nanos: quantile(&row.latency, 0),
        p95_nanos: quantile(&row.latency, 1),
        p99_nanos: quantile(&row.latency, 2),
        last_seen_unix_nanos: unsigned(row.last_seen),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn quantile(latency: &[f64], at: usize) -> u64 {
    let Some(value) = latency.get(at).copied().filter(|value| value.is_finite()) else {
        return 0;
    };
    value.clamp(0.0, u64::MAX as f64) as u64
}

fn ordering(order: TraceOrder) -> &'static str {
    match order {
        TraceOrder::Started => "started_at",
        TraceOrder::Duration => "duration",
        TraceOrder::Spans => "spans",
    }
}

fn read_summary(row: SummaryRow) -> Result<TraceSummary, StoreError> {
    Ok(TraceSummary {
        trace_id: a_trace(&row.trace_id)?,
        root_service: row.root_service.into(),
        root_name: row.root_name.into(),
        started_unix_nanos: unsigned(row.started_at),
        duration_nanos: unsigned(row.duration),
        spans: row.spans,
        errors: row.errors,
    })
}

fn read_span(trace: TraceId, row: SpanRow, events: Vec<SpanEvent>) -> Result<Span, StoreError> {
    let parent = match row.parent_span_id.as_deref() {
        None | Some("") => None,
        Some(spelling) => Some(a_span(spelling)?),
    };
    Ok(Span {
        trace_id: trace,
        span_id: a_span(&row.span_id)?,
        parent,
        name: row.name.into(),
        service_name: row.service_name.into(),
        kind: read_kind(&row.kind),
        status: read_status(&row.status),
        start_unix_nanos: unsigned(row.start_time),
        end_unix_nanos: unsigned(row.end_time),
        attributes: map_of(row.attributes),
        events,
    })
}

fn map_of(pairs: Vec<(String, String)>) -> BTreeMap<Box<str>, Box<str>> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect()
}

pub(crate) fn a_trace(spelling: &str) -> Result<TraceId, StoreError> {
    TraceId::try_from(decoded(spelling)?.as_slice()).map_err(|error| held(&error))
}

pub(crate) fn a_span(spelling: &str) -> Result<SpanId, StoreError> {
    SpanId::try_from(decoded(spelling)?.as_slice()).map_err(|error| held(&error))
}

fn decoded(spelling: &str) -> Result<Vec<u8>, StoreError> {
    hex::decode(spelling).map_err(|error| held(&error))
}

pub(crate) fn held(error: &dyn core::fmt::Display) -> StoreError {
    tracing::error!(%error, "the span store holds an identifier we cannot read");
    StoreError::Unreachable
}

fn read_kind(spelling: &str) -> SpanKind {
    match spelling {
        "internal" => SpanKind::Internal,
        "server" => SpanKind::Server,
        "client" => SpanKind::Client,
        "producer" => SpanKind::Producer,
        "consumer" => SpanKind::Consumer,
        _ => SpanKind::Unspecified,
    }
}

fn read_status(spelling: &str) -> SpanStatus {
    match spelling {
        "ok" => SpanStatus::Ok,
        "error" => SpanStatus::Error,
        _ => SpanStatus::Unset,
    }
}

pub(crate) fn uuid_of(project: Option<ProjectId>) -> Uuid {
    project.map(ProjectId::as_uuid).unwrap_or_default()
}

pub(crate) fn nanos(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

pub(crate) fn refused(error: &clickhouse::error::Error) -> StoreError {
    tracing::error!(%error, "the span store refused a query");
    StoreError::Unreachable
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{SpanKind, SpanStatus, TraceOrder};

    use super::{ONE_TRACE, SUMMARIES, ordering, read_kind, read_status};

    #[test]
    fn every_kind_the_schema_holds_is_read_back() {
        assert_eq!(read_kind("server"), SpanKind::Server);
        assert_eq!(read_kind("client"), SpanKind::Client);
        assert_eq!(read_kind("internal"), SpanKind::Internal);
        assert_eq!(read_kind("producer"), SpanKind::Producer);
        assert_eq!(read_kind("consumer"), SpanKind::Consumer);
        assert_eq!(read_kind("unspecified"), SpanKind::Unspecified);
    }

    #[test]
    fn every_status_the_schema_holds_is_read_back() {
        assert_eq!(read_status("ok"), SpanStatus::Ok);
        assert_eq!(read_status("error"), SpanStatus::Error);
        assert_eq!(read_status("unset"), SpanStatus::Unset);
    }

    #[test]
    fn a_kind_or_status_we_do_not_know_falls_back_rather_than_failing() {
        assert_eq!(read_kind("whatever"), SpanKind::Unspecified);
        assert_eq!(read_status("whatever"), SpanStatus::Unset);
    }

    #[test]
    fn every_ordering_names_a_column_the_query_selects() {
        for order in [TraceOrder::Started, TraceOrder::Duration, TraceOrder::Spans] {
            let column = ordering(order);
            assert!(
                SUMMARIES.contains(&format!("AS {column}")),
                "{column} is not selected"
            );
        }
    }

    #[test]
    fn neither_query_reads_across_tenants() {
        for query in [SUMMARIES, ONE_TRACE] {
            assert!(
                query.contains("WHERE tenant_id = ?"),
                "a query without a tenant would read somebody else's spans"
            );
        }
    }
}
