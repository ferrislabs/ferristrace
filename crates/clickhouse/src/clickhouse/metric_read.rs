use std::collections::BTreeMap;

use clickhouse::query::Query;
use clickhouse::{Client, Row};
use ferristrace_domain::{
    Aggregation, AttributeFilter, AttributeKey, Exemplar, HistogramPoint, MAX_ATTRIBUTES,
    MAX_EXEMPLARS, MAX_VALUES, MetricDescriptor, MetricKind, MetricQuery, MetricReader, ProjectId,
    SeenAttribute, SeenValue, Series, SeriesSet, SpanId, StoreError, TenantId, TraceId,
};
use serde::Deserialize;

use crate::clickhouse::placement::{Held, Placed};
use crate::clickhouse::query::Compiled;
use crate::clickhouse::read::{
    AttributeRow, ValueRow, held, map_of, nanos, refused, unsigned, uuid_of,
};

const MAX_CATALOGUE: u32 = 1000;
const NANOS_PER_SECOND: f64 = 1_000_000_000.0;
const ZERO_TRACE: &str = "00000000000000000000000000000000";
const ZERO_SPAN: &str = "0000000000000000";

const SEEN: &str = "(SELECT attributes, time FROM metric_points \
     WHERE tenant_id = ? AND name = ? \
       AND time >= fromUnixTimestamp64Nano(?) AND time < fromUnixTimestamp64Nano(?) \
       AND (? = 0 OR project_id = ?) \
     UNION ALL \
     SELECT attributes, time FROM metric_histograms \
     WHERE tenant_id = ? AND name = ? \
       AND time >= fromUnixTimestamp64Nano(?) AND time < fromUnixTimestamp64Nano(?) \
       AND (? = 0 OR project_id = ?))";

const ATTRIBUTES: &str = "SELECT key, count() AS spans, \
     countIf(toFloat64OrNull(value) IS NOT NULL) AS numbers \
   FROM {seen} \
   ARRAY JOIN mapKeys(attributes) AS key, mapValues(attributes) AS value \
   WHERE (? = '' OR positionCaseInsensitive(key, ?) > 0) \
   GROUP BY key \
   ORDER BY spans DESC, key \
   LIMIT ?";

const ATTRIBUTE_VALUES: &str = "SELECT attributes[?] AS value, count() AS spans \
   FROM {seen} \
   WHERE mapContains(attributes, ?) \
   GROUP BY value \
   HAVING (? = '' OR positionCaseInsensitive(value, ?) > 0) \
   ORDER BY spans DESC, value \
   LIMIT ?";

const CATALOGUE: &str = "SELECT \
     name AS name, \
     argMax(toString(kind), last_seen) AS kind, \
     argMax(unit, last_seen) AS unit, \
     argMax(description, last_seen) AS description, \
     toUnixTimestamp64Nano(max(last_seen)) AS last_seen_ns \
   FROM metric_catalogue FINAL \
   WHERE {filters} \
   GROUP BY name \
   ORDER BY name \
   LIMIT {limit}";

const POINTS: &str = "SELECT slot, labels, toNullable({outer}(per_series)) AS value FROM ( \
     SELECT slot, arrayMap(k -> attributes[k], CAST(? AS Array(String))) AS labels, {per_series} AS per_series FROM ( \
       SELECT slot, attributes, temporality, total, mean, low, high, last, first, samples, \
         row_number() OVER series AS nth, \
         lagInFrame(last) OVER series AS previous, \
         lagInFrame(slot) OVER series AS previous_slot \
       FROM ( \
         SELECT intDiv(toUnixTimestamp64Nano(time) - {from}, {step}) AS slot, \
           project_id, service_name, attributes, temporality, \
           sum(value) AS total, avg(value) AS mean, min(value) AS low, max(value) AS high, \
           argMax(value, time) AS last, argMin(value, time) AS first, count() AS samples \
         FROM metric_points \
         WHERE {filters} \
         GROUP BY slot, project_id, service_name, attributes, temporality \
       ) \
       WINDOW series AS (PARTITION BY project_id, service_name, attributes, temporality ORDER BY slot \
         ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) \
     ) \
   ) \
   GROUP BY slot, labels";

const CUMULATIVE_RATE: &str = "if(nth = 1, \
     if(samples > 1, greatest(last - first, 0) / {seconds}, NULL), \
     greatest(last - previous, 0) / ((slot - previous_slot) * {seconds}))";

const HISTOGRAMS: &str = "SELECT slot, labels, bounds, \
     sumForEach(step_counts) AS merged_counts, \
     sum(step_count) AS merged_count, \
     sum(step_sum) AS merged_sum, \
     min(low) AS merged_min, \
     max(high) AS merged_max, \
     sum(per_series_rate) AS merged_rate \
   FROM ( \
     SELECT slot, bounds, low, high, \
       arrayMap(k -> attributes[k], CAST(? AS Array(String))) AS labels, \
       temporality = 'cumulative' AS cumulative, \
       if(nth = 1, first_count, previous_count) AS base_count, \
       if(nth = 1, first_sum, previous_sum) AS base_sum, \
       if(nth = 1, first_counts, previous_counts) AS base_counts, \
       if(cumulative, if(last_count >= base_count, toUInt64(last_count - base_count), 0), \
         total_count) AS step_count, \
       if(cumulative, greatest(last_sum - base_sum, 0), total_sum) AS step_sum, \
       if(cumulative, \
         if(length(base_counts) = length(last_counts), \
           arrayMap((a, b) -> if(a >= b, toUInt64(a - b), toUInt64(0)), last_counts, base_counts), \
           last_counts), \
         merged) AS step_counts, \
       if(cumulative, \
         if(nth = 1, \
           if(samples > 1, step_count / {seconds}, NULL), \
           step_count / ((slot - previous_slot) * {seconds})), \
         total_count / {seconds}) AS per_series_rate \
     FROM ( \
       SELECT *, \
         row_number() OVER series AS nth, \
         lagInFrame(last_count) OVER series AS previous_count, \
         lagInFrame(last_sum) OVER series AS previous_sum, \
         lagInFrame(last_counts) OVER series AS previous_counts, \
         lagInFrame(slot) OVER series AS previous_slot \
       FROM ( \
         SELECT intDiv(toUnixTimestamp64Nano(time) - {from}, {step}) AS slot, \
           project_id, service_name, attributes, temporality, bounds, \
           sum(count) AS total_count, sum(sum) AS total_sum, sumForEach(counts) AS merged, \
           min(min) AS low, max(max) AS high, \
           argMax(count, time) AS last_count, argMin(count, time) AS first_count, \
           argMax(sum, time) AS last_sum, argMin(sum, time) AS first_sum, \
           argMax(counts, time) AS last_counts, argMin(counts, time) AS first_counts, \
           count() AS samples \
         FROM metric_histograms \
         WHERE {filters} \
         GROUP BY slot, project_id, service_name, attributes, temporality, bounds \
       ) \
       WINDOW series AS (PARTITION BY project_id, service_name, attributes, temporality, bounds \
         ORDER BY slot \
         ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) \
     ) \
   ) \
   GROUP BY slot, labels, bounds";

const EXEMPLARS: &str = "SELECT \
     toUnixTimestamp64Nano(time) AS time_ns, \
     value AS value, \
     lower(hex(trace_id)) AS trace_hex, \
     lower(hex(span_id)) AS span_hex, \
     attributes AS attributes \
   FROM metric_exemplars \
   WHERE {filters} \
   ORDER BY time DESC \
   LIMIT {limit}";

#[derive(Debug, Clone)]
pub struct ClickhouseMetricReader {
    client: Client,
}

impl ClickhouseMetricReader {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub(crate) fn client(&self) -> &Client {
        &self.client
    }
}

#[derive(Debug, Row, Deserialize)]
struct CatalogueRow {
    name: String,
    kind: String,
    unit: String,
    description: String,
    last_seen_ns: i64,
}

#[derive(Debug, Row, Deserialize)]
struct PointRow {
    slot: i64,
    labels: Vec<String>,
    value: Option<f64>,
}

#[derive(Debug, Row, Deserialize)]
struct HistogramRow {
    slot: i64,
    labels: Vec<String>,
    bounds: Vec<f64>,
    merged_counts: Vec<u64>,
    merged_count: u64,
    merged_sum: f64,
    merged_min: Option<f64>,
    merged_max: Option<f64>,
    merged_rate: Option<f64>,
}

#[derive(Debug, Row, Deserialize)]
struct ExemplarRow {
    time_ns: i64,
    value: f64,
    trace_hex: String,
    span_hex: String,
    attributes: Vec<(String, String)>,
}

fn scoped(project: Option<ProjectId>) -> &'static str {
    if project.is_some() {
        "tenant_id = ? AND project_id = ?"
    } else {
        "tenant_id = ?"
    }
}

fn bind_scope(query: Query, org: TenantId, project: Option<ProjectId>) -> Query {
    let query = query.bind(org.as_uuid());
    match project {
        Some(project) => query.bind(project.as_uuid()),
        None => query,
    }
}

fn charted(query: &MetricQuery, compiled: &Compiled) -> String {
    let narrowed = if compiled.is_empty() {
        String::new()
    } else {
        format!(" AND ({})", compiled.expression())
    };
    format!(
        "{} AND name = ? \
         AND time >= fromUnixTimestamp64Nano(toInt64({})) \
         AND time < fromUnixTimestamp64Nano(toInt64({})){narrowed}",
        scoped(query.project),
        nanos(query.window.from_unix_nanos()),
        nanos(query.window.to_unix_nanos()),
    )
}

fn bind_filter(mut statement: Query, compiled: &Compiled) -> Query {
    for param in compiled.params() {
        statement = statement.bind(param);
    }
    statement
}

fn laid(
    sql: &str,
    query: &MetricQuery,
    placed: &Placed<'_>,
    compiled: &Compiled,
    per_series: &str,
    outer: &str,
) -> String {
    #[allow(clippy::cast_precision_loss)]
    let seconds = query.step_nanos() as f64 / NANOS_PER_SECOND;
    sql.replace("{per_series}", per_series)
        .replace("{outer}", outer)
        .replace("{seconds}", &format!("{seconds:?}"))
        .replace("{from}", &nanos(query.window.from_unix_nanos()).to_string())
        .replace("{step}", &nanos(query.step_nanos()).to_string())
        .replace(
            "{filters}",
            &format!("{}{}", charted(query, compiled), placed.and()),
        )
}

fn point_plan(kind: MetricKind, aggregation: Aggregation) -> (String, &'static str) {
    let outer = match aggregation {
        Aggregation::Avg => "avg",
        Aggregation::Min => "min",
        Aggregation::Max => "max",
        Aggregation::Sum
        | Aggregation::Rate
        | Aggregation::P50
        | Aggregation::P95
        | Aggregation::P99 => "sum",
    };
    let per_series = match (kind, aggregation) {
        (MetricKind::Gauge, Aggregation::Avg) => "mean".to_owned(),
        (MetricKind::Gauge, Aggregation::Min) => "low".to_owned(),
        (MetricKind::Gauge, Aggregation::Max) => "high".to_owned(),
        (MetricKind::Gauge, _) => "last".to_owned(),
        (_, Aggregation::Rate) => {
            format!("if(temporality = 'cumulative', {CUMULATIVE_RATE}, total / {{seconds}})")
        }
        (_, _) => "if(temporality = 'cumulative', last, total)".to_owned(),
    };
    (per_series, outer)
}

impl ClickhouseMetricReader {
    async fn points(
        &self,
        org: TenantId,
        kind: MetricKind,
        aggregation: Aggregation,
        query: &MetricQuery,
    ) -> Result<BTreeMap<Vec<String>, Vec<Option<f64>>>, StoreError> {
        let (per_series, outer) = point_plan(kind, aggregation);
        let placed = Placed::of(&query.placement, Held::Attributes);
        let compiled = Compiled::of_metrics(&query.filter);
        let sql = laid(POINTS, query, &placed, &compiled, &per_series, outer);
        let rows: Vec<PointRow> = placed
            .bind(bind_filter(
                bind_scope(
                    self.client.query(&sql).bind(keys_of(query)),
                    org,
                    query.project,
                )
                .bind(&*query.name),
                &compiled,
            ))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let mut laid_out = BTreeMap::new();
        for row in rows {
            place(&mut laid_out, query, row.slot, row.labels, row.value);
        }
        Ok(laid_out)
    }

    async fn histograms(
        &self,
        org: TenantId,
        aggregation: Aggregation,
        query: &MetricQuery,
    ) -> Result<BTreeMap<Vec<String>, Vec<Option<f64>>>, StoreError> {
        let placed = Placed::of(&query.placement, Held::Attributes);
        let compiled = Compiled::of_metrics(&query.filter);
        let sql = laid(HISTOGRAMS, query, &placed, &compiled, "", "");
        let rows: Vec<HistogramRow> = placed
            .bind(bind_filter(
                bind_scope(
                    self.client.query(&sql).bind(keys_of(query)),
                    org,
                    query.project,
                )
                .bind(&*query.name),
                &compiled,
            ))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let mut fullest: BTreeMap<(Vec<String>, i64), HistogramRow> = BTreeMap::new();
        for row in rows {
            let key = (row.labels.clone(), row.slot);
            match fullest.get(&key) {
                Some(held) if held.merged_count >= row.merged_count => {}
                _ => {
                    fullest.insert(key, row);
                }
            }
        }
        let mut laid_out = BTreeMap::new();
        for ((labels, slot), row) in fullest {
            let value = histogram_value(aggregation, row);
            place(&mut laid_out, query, slot, labels, value);
        }
        Ok(laid_out)
    }

    async fn exemplars(
        &self,
        org: TenantId,
        query: &MetricQuery,
    ) -> Result<Vec<Exemplar>, StoreError> {
        if !query.filter.is_anything() {
            return Ok(Vec::new());
        }
        let placed = Placed::of(&query.placement, Held::Attributes);
        let touched = if placed.is_empty() {
            String::new()
        } else {
            format!(
                " AND (trace_id, span_id) IN ({})",
                placed.spans("trace_id, span_id")
            )
        };
        let sql = EXEMPLARS
            .replace(
                "{filters}",
                &format!("{}{touched}", charted(query, &Compiled::default())),
            )
            .replace("{limit}", &MAX_EXEMPLARS.to_string());
        let mut asked = bind_scope(self.client.query(&sql), org, query.project).bind(&*query.name);
        if !placed.is_empty() {
            asked = placed.bind_spans(asked, org, query.window, query.project);
        }
        let rows: Vec<ExemplarRow> = asked.fetch_all().await.map_err(|error| refused(&error))?;
        rows.into_iter()
            .map(|row| {
                Ok(Exemplar {
                    time_unix_nanos: unsigned(row.time_ns),
                    value: row.value,
                    trace_id: optional(&row.trace_hex, ZERO_TRACE, |bytes| {
                        TraceId::try_from(bytes)
                    })?,
                    span_id: optional(&row.span_hex, ZERO_SPAN, |bytes| SpanId::try_from(bytes))?,
                    attributes: map_of(row.attributes),
                })
            })
            .collect()
    }
}

fn keys_of(query: &MetricQuery) -> Vec<&str> {
    query.group_by.iter().map(|key| &**key).collect()
}

fn place(
    laid_out: &mut BTreeMap<Vec<String>, Vec<Option<f64>>>,
    query: &MetricQuery,
    slot: i64,
    labels: Vec<String>,
    value: Option<f64>,
) {
    let slots = usize::try_from(query.slots()).unwrap_or(usize::MAX);
    let Some(index) = usize::try_from(slot).ok().filter(|index| *index < slots) else {
        return;
    };
    let points = laid_out.entry(labels).or_insert_with(|| vec![None; slots]);
    points[index] = value.filter(|number| number.is_finite());
}

#[allow(clippy::cast_precision_loss)]
fn histogram_value(aggregation: Aggregation, row: HistogramRow) -> Option<f64> {
    match aggregation {
        Aggregation::Avg => {
            (row.merged_count > 0).then(|| row.merged_sum / row.merged_count as f64)
        }
        Aggregation::Rate => row.merged_rate,
        _ => {
            let quantile = aggregation.quantile()?;
            HistogramPoint {
                time_unix_nanos: 0,
                count: row.merged_count,
                sum: row.merged_sum,
                min: row.merged_min,
                max: row.merged_max,
                bounds: row.bounds,
                counts: row.merged_counts,
                attributes: BTreeMap::new(),
                exemplars: Vec::new(),
            }
            .quantile(quantile)
        }
    }
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

fn bind_seen(statement: Query, org: TenantId, metric: &str, filter: &AttributeFilter) -> Query {
    let mut statement = statement;
    for _ in 0..2 {
        statement = statement
            .bind(org.as_uuid())
            .bind(metric)
            .bind(nanos(filter.window.from_unix_nanos()))
            .bind(nanos(filter.window.to_unix_nanos()))
            .bind(u8::from(filter.project.is_some()))
            .bind(uuid_of(filter.project));
    }
    statement
}

impl MetricReader for ClickhouseMetricReader {
    async fn attributes(
        &self,
        org: TenantId,
        metric: &str,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenAttribute>, StoreError> {
        let prefix = filter.prefix.as_deref().unwrap_or_default();
        let sql = ATTRIBUTES.replace("{seen}", SEEN);
        let rows: Vec<AttributeRow> = bind_seen(self.client.query(&sql), org, metric, filter)
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
        metric: &str,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenValue>, StoreError> {
        let prefix = filter.prefix.as_deref().unwrap_or_default();
        let sql = ATTRIBUTE_VALUES.replace("{seen}", SEEN);
        let rows: Vec<ValueRow> = bind_seen(
            self.client.query(&sql).bind(key.as_str()),
            org,
            metric,
            filter,
        )
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

    async fn catalogue(
        &self,
        org: TenantId,
        search: Option<&str>,
        project: Option<ProjectId>,
    ) -> Result<Vec<MetricDescriptor>, StoreError> {
        let mut filters = scoped(project).to_owned();
        if search.is_some() {
            filters.push_str(" AND positionCaseInsensitive(name, ?) > 0");
        }
        let sql = CATALOGUE
            .replace("{filters}", &filters)
            .replace("{limit}", &MAX_CATALOGUE.to_string());
        let mut asked = bind_scope(self.client.query(&sql), org, project);
        if let Some(text) = search {
            asked = asked.bind(text);
        }
        let rows: Vec<CatalogueRow> = asked.fetch_all().await.map_err(|error| refused(&error))?;
        rows.into_iter()
            .map(|row| {
                Ok(MetricDescriptor {
                    kind: MetricKind::try_from(row.kind.as_str()).map_err(|error| held(&error))?,
                    name: row.name.into(),
                    unit: row.unit.into(),
                    description: row.description.into(),
                    last_seen_unix_nanos: unsigned(row.last_seen_ns),
                })
            })
            .collect()
    }

    async fn series(
        &self,
        org: TenantId,
        kind: MetricKind,
        aggregation: Aggregation,
        query: &MetricQuery,
    ) -> Result<SeriesSet, StoreError> {
        let laid_out = match kind {
            MetricKind::Histogram => self.histograms(org, aggregation, query).await?,
            MetricKind::Gauge | MetricKind::Sum => {
                self.points(org, kind, aggregation, query).await?
            }
        };
        let starts: Vec<u64> = query.slot_starts().collect();
        let series = laid_out
            .into_iter()
            .map(|(labels, values)| Series {
                labels: query
                    .group_by
                    .iter()
                    .cloned()
                    .zip(labels.into_iter().map(String::into_boxed_str))
                    .collect(),
                points: starts.iter().copied().zip(values).collect(),
            })
            .collect();
        Ok(SeriesSet {
            step_nanos: query.step_nanos(),
            series,
            exemplars: self.exemplars(org, query).await?,
        })
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{Aggregation, MetricKind};

    use super::point_plan;

    #[test]
    fn a_gauge_is_read_through_its_own_statistic_per_series() {
        assert_eq!(
            point_plan(MetricKind::Gauge, Aggregation::Avg),
            ("mean".to_owned(), "avg")
        );
        assert_eq!(
            point_plan(MetricKind::Gauge, Aggregation::Sum),
            ("last".to_owned(), "sum")
        );
        assert_eq!(
            point_plan(MetricKind::Gauge, Aggregation::Max),
            ("high".to_owned(), "max")
        );
    }

    #[test]
    fn a_counter_rate_tells_delta_from_cumulative_and_sums_its_series() {
        let (per_series, outer) = point_plan(MetricKind::Sum, Aggregation::Rate);

        assert_eq!(outer, "sum");
        assert!(
            per_series.contains("temporality = 'cumulative'"),
            "{per_series}"
        );
        assert!(
            per_series.contains("greatest(last - previous, 0)"),
            "{per_series}"
        );
        assert!(per_series.contains("total / {seconds}"), "{per_series}");
    }
}
