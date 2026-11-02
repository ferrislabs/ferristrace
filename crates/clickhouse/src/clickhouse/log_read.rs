use clickhouse::query::Query;
use clickhouse::{Client, Row};
use ferristrace_domain::{
    Buckets, LogBucket, LogCursor, LogPage, LogQuery, LogReader, LogRecord, Severity, SpanId,
    StoreError, TenantId, TraceId,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::clickhouse::read::{held, map_of, nanos, refused, unsigned};

const TIEBREAK: &str = "cityHash64(observed_time, body, trace_id, span_id, attributes)";

const RECORDS: &str = "SELECT \
     toUnixTimestamp64Nano(time) AS time_ns, \
     toUnixTimestamp64Nano(observed_time) AS observed_ns, \
     toString(severity) AS level, \
     severity_number AS severity_number, \
     service_name AS service, \
     body AS body, \
     attributes AS attributes, \
     resource AS resource, \
     lower(hex(trace_id)) AS trace_hex, \
     lower(hex(span_id)) AS span_hex, \
     {tiebreak} AS tiebreak \
   FROM logs \
   WHERE {filters} \
   ORDER BY time DESC, tiebreak DESC \
   LIMIT ?";

const COUNTS: &str = "SELECT \
     intDiv(toUnixTimestamp64Nano(time) - ?, ?) AS slot, \
     toString(severity) AS level, \
     count() AS hits \
   FROM logs \
   WHERE {filters} \
   GROUP BY slot, level";

const ZERO_TRACE: &str = "00000000000000000000000000000000";
const ZERO_SPAN: &str = "0000000000000000";

#[derive(Debug, Clone)]
pub struct ClickhouseLogReader {
    client: Client,
}

impl ClickhouseLogReader {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[derive(Debug, Row, Deserialize)]
struct RecordRow {
    time_ns: i64,
    observed_ns: i64,
    level: String,
    severity_number: u8,
    service: String,
    body: String,
    attributes: Vec<(String, String)>,
    resource: Vec<(String, String)>,
    trace_hex: String,
    span_hex: String,
    tiebreak: u64,
}

#[derive(Debug, Row, Deserialize)]
struct CountRow {
    slot: i64,
    level: String,
    hits: u64,
}

enum Bound<'a> {
    Tenant(Uuid),
    Nanos(i64),
    Count(u64),
    Text(&'a str),
    Texts(Vec<&'a str>),
    Hex(String),
}

struct Filters<'a> {
    clauses: Vec<String>,
    bound: Vec<Bound<'a>>,
}

impl<'a> Filters<'a> {
    fn of(org: TenantId, query: &'a LogQuery, paged: bool) -> Self {
        let mut filters = Self {
            clauses: Vec::new(),
            bound: Vec::new(),
        };
        filters.push("tenant_id = ?", [Bound::Tenant(org.as_uuid())]);
        filters.push(
            "time >= fromUnixTimestamp64Nano(?) AND time < fromUnixTimestamp64Nano(?)",
            [
                Bound::Nanos(nanos(query.window.from_unix_nanos())),
                Bound::Nanos(nanos(query.window.to_unix_nanos())),
            ],
        );
        if let Some(project) = query.project {
            filters.push("project_id = ?", [Bound::Tenant(project.as_uuid())]);
        }
        if !query.services.is_empty() {
            filters.push(
                "has(?, service_name)",
                [Bound::Texts(query.services.iter().map(|s| &**s).collect())],
            );
        }
        if !query.severities.is_empty() {
            filters.push(
                "has(?, toString(severity))",
                [Bound::Texts(
                    query.severities.iter().map(|s| s.name()).collect(),
                )],
            );
        }
        if let Some(text) = query.text.as_deref() {
            filters.push("positionCaseInsensitive(body, ?) > 0", [Bound::Text(text)]);
        }
        for (key, value) in &query.attributes {
            filters.push("attributes[?] = ?", [Bound::Text(key), Bound::Text(value)]);
        }
        if let Some(trace) = query.trace {
            filters.push(
                "trace_id = unhex(?)",
                [Bound::Hex(hex::encode(trace.as_bytes()))],
            );
        }
        if let (true, Some(before)) = (paged, query.before) {
            filters.clauses.push(format!(
                "(time, {TIEBREAK}) < (fromUnixTimestamp64Nano(?), ?)"
            ));
            filters
                .bound
                .push(Bound::Nanos(nanos(before.time_unix_nanos)));
            filters.bound.push(Bound::Count(before.tiebreak));
        }
        filters
    }

    fn push<const N: usize>(&mut self, clause: &str, bound: [Bound<'a>; N]) {
        self.clauses.push(clause.to_owned());
        self.bound.extend(bound);
    }

    fn sql(&self, template: &str) -> String {
        template
            .replace("{tiebreak}", TIEBREAK)
            .replace("{filters}", &self.clauses.join(" AND "))
    }

    fn bind(self, mut query: Query) -> Query {
        for bound in self.bound {
            query = match bound {
                Bound::Tenant(id) => query.bind(id),
                Bound::Nanos(at) => query.bind(at),
                Bound::Count(count) => query.bind(count),
                Bound::Text(text) => query.bind(text),
                Bound::Texts(texts) => query.bind(texts),
                Bound::Hex(spelling) => query.bind(spelling),
            };
        }
        query
    }
}

impl LogReader for ClickhouseLogReader {
    async fn search(&self, org: TenantId, query: &LogQuery) -> Result<LogPage, StoreError> {
        let filters = Filters::of(org, query, true);
        let sql = filters.sql(RECORDS);
        let limit = query.limit();
        let rows: Vec<RecordRow> = filters
            .bind(self.client.query(&sql))
            .bind(u64::from(limit) + 1)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let more = rows.len() > limit as usize;
        let mut records = Vec::with_capacity(rows.len().min(limit as usize));
        let mut next = None;
        for row in rows.into_iter().take(limit as usize) {
            next = Some(LogCursor {
                time_unix_nanos: unsigned(row.time_ns),
                tiebreak: row.tiebreak,
            });
            records.push(read_record(row)?);
        }
        Ok(LogPage {
            records,
            next: next.filter(|_| more),
        })
    }

    async fn histogram(
        &self,
        org: TenantId,
        query: &LogQuery,
        buckets: Buckets,
    ) -> Result<Vec<LogBucket>, StoreError> {
        let width = buckets.width_over(query.window);
        let filters = Filters::of(org, query, false);
        let sql = filters.sql(COUNTS);
        let rows: Vec<CountRow> = filters
            .bind(
                self.client
                    .query(&sql)
                    .bind(nanos(query.window.from_unix_nanos()))
                    .bind(width),
            )
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let mut laid = buckets.laid_over(query.window);
        for row in rows {
            let Some(bucket) = usize::try_from(row.slot)
                .ok()
                .and_then(|slot| laid.get_mut(slot))
            else {
                continue;
            };
            bucket.add(read_severity(&row.level), row.hits);
        }
        Ok(laid)
    }
}

fn read_record(row: RecordRow) -> Result<LogRecord, StoreError> {
    Ok(LogRecord {
        time_unix_nanos: unsigned(row.time_ns),
        observed_unix_nanos: unsigned(row.observed_ns),
        severity: read_severity(&row.level),
        severity_number: row.severity_number,
        service_name: row.service.into(),
        body: row.body.into(),
        attributes: map_of(row.attributes),
        resource: map_of(row.resource),
        trace_id: optional(&row.trace_hex, ZERO_TRACE, |bytes| TraceId::try_from(bytes))?,
        span_id: optional(&row.span_hex, ZERO_SPAN, |bytes| SpanId::try_from(bytes))?,
    })
}

fn optional<T, E: core::fmt::Display>(
    spelling: &str,
    zero: &str,
    read: impl Fn(&[u8]) -> Result<T, E>,
) -> Result<Option<T>, StoreError> {
    if spelling.is_empty() || spelling == zero {
        return Ok(None);
    }
    let bytes = hex::decode(spelling).map_err(|error| held(&error))?;
    read(&bytes).map(Some).map_err(|error| held(&error))
}

fn read_severity(spelling: &str) -> Severity {
    Severity::try_from(spelling).unwrap_or(Severity::Info)
}
