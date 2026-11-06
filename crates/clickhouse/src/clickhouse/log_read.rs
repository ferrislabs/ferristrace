use clickhouse::query::Query;
use clickhouse::{Client, Row};
use ferristrace_domain::{
    AttributeFilter, AttributeKey, Buckets, CulpritPod, CulpritReader, Culprits, FACET_VALUES,
    FacetFilter, FacetValues, Facets, Labels, LogBucket, LogCursor, LogPage, LogQuery, LogReader,
    LogRecord, MAX_ATTRIBUTES, MAX_CULPRIT_FACETS, MAX_CULPRIT_PODS, MAX_VALUES, ProjectId,
    SeenAttribute, SeenValue, Severity, Signal, SpanId, StoreError, TenantId, TraceId, Window,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::clickhouse::placement::{Held, Placed};
use crate::clickhouse::query::{Compiled, Param, Table};
use crate::clickhouse::read::{
    AttributeRow, ValueRow, held, map_of, nanos, refused, unsigned, uuid_of,
};

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

const CULPRIT_PODS: &str = "SELECT k8s_pod AS value, \
     argMaxIf(k8s_node, time, k8s_node != '') AS node, \
     count() AS hits \
   FROM logs WHERE {filters} AND k8s_pod != '' \
   GROUP BY k8s_pod ORDER BY hits DESC, value LIMIT {pods}";

const CULPRIT_PODS_SEEN: &str = "SELECT uniqExact(k8s_pod) AS total \
   FROM logs WHERE {filters} AND k8s_pod != ''";

const CULPRIT_NODES: &str = "SELECT k8s_node AS value, '' AS node, count() AS hits \
   FROM logs WHERE {filters} AND k8s_node != '' \
   GROUP BY k8s_node ORDER BY hits DESC, value LIMIT {facets}";

const CULPRIT_VERSIONS: &str = "SELECT resource['service.version'] AS value, '' AS node, count() AS hits \
   FROM logs WHERE {filters} AND resource['service.version'] != '' \
   GROUP BY value ORDER BY hits DESC, value LIMIT {facets}";

const ATTRIBUTES: &str = "SELECT key, count() AS spans, \
     countIf(toFloat64OrNull(value) IS NOT NULL) AS numbers \
   FROM logs \
   ARRAY JOIN mapKeys(mapUpdate(resource, attributes)) AS key, \
              mapValues(mapUpdate(resource, attributes)) AS value \
   WHERE tenant_id = ? \
     AND time >= fromUnixTimestamp64Nano(?) AND time < fromUnixTimestamp64Nano(?) \
     AND (? = 0 OR project_id = ?) \
     AND (? = '' OR positionCaseInsensitive(key, ?) > 0) \
   GROUP BY key \
   ORDER BY spans DESC, key \
   LIMIT ?";

const ATTRIBUTE_VALUES: &str = "SELECT mapUpdate(resource, attributes)[?] AS value, count() AS spans \
   FROM logs \
   WHERE tenant_id = ? \
     AND time >= fromUnixTimestamp64Nano(?) AND time < fromUnixTimestamp64Nano(?) \
     AND (? = 0 OR project_id = ?) \
     AND (mapContains(attributes, ?) OR mapContains(resource, ?)) \
   GROUP BY value \
   HAVING (? = '' OR positionCaseInsensitive(value, ?) > 0) \
   ORDER BY spans DESC, value \
   LIMIT ?";

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
struct CulpritRow {
    value: String,
    node: String,
    hits: u64,
}

#[derive(Debug, Row, Deserialize)]
struct SeenRow {
    total: u64,
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
    Param(&'a Param),
}

struct Filters<'a> {
    clauses: Vec<String>,
    bound: Vec<Bound<'a>>,
}

impl<'a> Filters<'a> {
    fn of(org: TenantId, query: &'a LogQuery, compiled: &'a Compiled, paged: bool) -> Self {
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
        let placed = Placed::of(&query.placement, Held::Resource);
        for (clause, value) in placed.clauses().iter().zip(placed.values()) {
            filters.push(clause, [Bound::Text(value)]);
        }
        if !compiled.is_empty() {
            filters.clauses.push(format!("({})", compiled.expression()));
            filters
                .bound
                .extend(compiled.params().iter().map(Bound::Param));
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
                Bound::Param(param) => query.bind(param),
            };
        }
        query
    }
}

impl ClickhouseLogReader {
    async fn culprit_rows(
        &self,
        template: &str,
        org: TenantId,
        query: &LogQuery,
    ) -> Result<Vec<CulpritRow>, StoreError> {
        let compiled = Compiled::of_logs(&query.query);
        let filters = Filters::of(org, query, &compiled, false);
        let sql = filters
            .sql(template)
            .replace("{pods}", &MAX_CULPRIT_PODS.to_string())
            .replace("{facets}", &MAX_CULPRIT_FACETS.to_string());
        filters
            .bind(self.client.query(&sql))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))
    }

    async fn pods_seen(&self, org: TenantId, query: &LogQuery) -> Result<u64, StoreError> {
        let compiled = Compiled::of_logs(&query.query);
        let filters = Filters::of(org, query, &compiled, false);
        let sql = filters.sql(CULPRIT_PODS_SEEN);
        let seen: SeenRow = filters
            .bind(self.client.query(&sql))
            .fetch_one()
            .await
            .map_err(|error| refused(&error))?;
        Ok(seen.total)
    }
}

impl CulpritReader for ClickhouseLogReader {
    async fn culprits(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        signal: &Signal,
        window: Window,
    ) -> Result<Culprits, StoreError> {
        let Signal::Logs(logs) = signal else {
            return Ok(Culprits::default());
        };
        let query = logs.query(project, window);
        let pods = self.culprit_rows(CULPRIT_PODS, org, &query).await?;
        let total = self.pods_seen(org, &query).await?;
        let nodes = self.culprit_rows(CULPRIT_NODES, org, &query).await?;
        let versions = self.culprit_rows(CULPRIT_VERSIONS, org, &query).await?;
        Ok(Culprits::new(
            pods.into_iter()
                .map(|row| CulpritPod {
                    name: row.value.into(),
                    node: Some(row.node)
                        .filter(|node| !node.is_empty())
                        .map(Into::into),
                    count: row.hits,
                })
                .collect(),
            total,
            nodes.into_iter().map(|row| row.value.into()).collect(),
            versions.into_iter().map(|row| row.value.into()).collect(),
            &Labels::none(),
        ))
    }
}

impl LogReader for ClickhouseLogReader {
    async fn search(&self, org: TenantId, query: &LogQuery) -> Result<LogPage, StoreError> {
        let compiled = Compiled::of_logs(&query.query);
        let filters = Filters::of(org, query, &compiled, true);
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
        let compiled = Compiled::of_logs(&query.query);
        let filters = Filters::of(org, query, &compiled, false);
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

    async fn attributes(
        &self,
        org: TenantId,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenAttribute>, StoreError> {
        let prefix = filter.prefix.as_deref().unwrap_or_default();
        let rows: Vec<AttributeRow> = self
            .client
            .query(ATTRIBUTES)
            .bind(org.as_uuid())
            .bind(nanos(filter.window.from_unix_nanos()))
            .bind(nanos(filter.window.to_unix_nanos()))
            .bind(u8::from(filter.project.is_some()))
            .bind(uuid_of(filter.project))
            .bind(prefix)
            .bind(prefix)
            .bind(MAX_ATTRIBUTES)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;
        Ok(rows
            .into_iter()
            .map(|row| SeenAttribute {
                numeric: row.numbers == row.spans,
                key: row.key.into(),
                spans: row.spans,
            })
            .collect())
    }

    async fn attribute_values(
        &self,
        org: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenValue>, StoreError> {
        let prefix = filter.prefix.as_deref().unwrap_or_default();
        let rows: Vec<ValueRow> = self
            .client
            .query(ATTRIBUTE_VALUES)
            .bind(key.as_str())
            .bind(org.as_uuid())
            .bind(nanos(filter.window.from_unix_nanos()))
            .bind(nanos(filter.window.to_unix_nanos()))
            .bind(u8::from(filter.project.is_some()))
            .bind(uuid_of(filter.project))
            .bind(key.as_str())
            .bind(key.as_str())
            .bind(prefix)
            .bind(prefix)
            .bind(MAX_VALUES)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;
        Ok(rows
            .into_iter()
            .map(|row| SeenValue {
                value: row.value.into(),
                spans: row.spans,
            })
            .collect())
    }

    async fn facets(&self, org: TenantId, filter: &FacetFilter) -> Result<Facets, StoreError> {
        let mut fields = Vec::with_capacity(filter.fields().len());
        for field in filter.fields() {
            let (sql, params) = Compiled::facet_in(
                Table::Logs,
                field,
                &filter.query,
                org,
                filter.window,
                filter.project,
                FACET_VALUES,
                true,
            );
            let mut statement = self.client.query(&sql);
            for param in &params {
                statement = statement.bind(param);
            }
            let rows: Vec<ValueRow> = statement
                .fetch_all()
                .await
                .map_err(|error| refused(&error))?;
            fields.push(FacetValues {
                field: field.clone(),
                values: rows
                    .into_iter()
                    .map(|row| SeenValue {
                        value: row.value.into(),
                        spans: row.spans,
                    })
                    .collect(),
            });
        }
        Ok(Facets {
            fields,
            duration: None,
        })
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
