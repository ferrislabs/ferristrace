use clickhouse::Row;
use ferristrace_domain::{
    RequestGrouping, RequestPoint, RequestSeries, RequestSummary, ServiceFilter, StoreError,
    TenantId,
};
use serde::Deserialize;

use crate::clickhouse::placement::{Held, Placed};
use crate::clickhouse::read::{
    ClickhouseReader, nanos, nanos_of, quantile, refused, unsigned, uuid_of,
};

const REQUESTED: &str = "FROM spans FINAL \
   WHERE tenant_id = ? \
     AND start_time >= fromUnixTimestamp64Nano(?) \
     AND start_time < fromUnixTimestamp64Nano(?) \
     AND kind = 'server' \
     AND (mapContains(attributes, 'http.request.method') \
       OR mapContains(attributes, 'http.method') \
       OR http_status_code IS NOT NULL) \
     AND (? = 0 OR project_id = ?) \
     AND (? = '' OR attributes['deployment.environment'] = ? \
       OR attributes['deployment.environment.name'] = ?){placed}";

const ROUTE: &str = "if(attributes['http.route'] != '', attributes['http.route'], name)";

const METHOD: &str = "multiIf(attributes['http.request.method'] != '', \
   attributes['http.request.method'], attributes['http.method'] != '', \
   attributes['http.method'], 'unknown')";

const BROKEN: &str = "countIf(http_status_code >= 500 \
   OR (http_status_code IS NULL AND status = 'error')) AS broken";

const COUNTED: &str = "count() AS requests, \
     countIf(http_status_code < 400) AS answered, \
     countIf(http_status_code >= 400 AND http_status_code < 500) AS refused, \
     {broken}, \
     sum(toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time)) AS total_nanos, \
     quantiles(0.5, 0.95)( \
       toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time) \
     ) AS latency, \
     toUnixTimestamp64Nano(max(start_time)) AS last_seen";

const BY_ROUTE: &str = "SELECT {route} AS route, service_name AS service, {method} AS method, \
     {counted} {requested} \
   GROUP BY route, service, method ORDER BY requests DESC, route, service, method LIMIT 500";

const BY_SERVICE: &str = "SELECT service_name AS route, service_name AS service, 'all' AS method, \
     {counted} {requested} \
   GROUP BY route, service ORDER BY requests DESC, route, service LIMIT 500";

const BY_METHOD: &str = "SELECT {method} AS route, 'all' AS service, {method} AS method, \
     {counted} {requested} \
   GROUP BY route, method ORDER BY requests DESC, route, method LIMIT 500";

const STEPS: &str = "SELECT \
     toInt64(intDiv(toUnixTimestamp64Nano(start_time) - {from}, {step})) AS slot, \
     count() AS requests, \
     countIf(http_status_code >= 400 AND http_status_code < 500) AS refused, \
     {broken}, \
     quantile(0.95)(toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time)) AS p95 \
   {requested} GROUP BY slot ORDER BY slot";

const WHOLE: &str = "SELECT \
     count() AS requests, \
     countIf(http_status_code < 400) AS answered, \
     countIf(http_status_code >= 400 AND http_status_code < 500) AS refused, \
     {broken}, \
     quantiles(0.5, 0.95, 0.99)( \
       toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time) \
     ) AS latency \
   {requested}";

#[derive(Debug, Row, Deserialize)]
struct RequestRow {
    route: String,
    service: String,
    method: String,
    requests: u64,
    answered: u64,
    refused: u64,
    broken: u64,
    total_nanos: i64,
    latency: Vec<f64>,
    last_seen: i64,
}

#[derive(Debug, Row, Deserialize)]
struct RequestStepRow {
    slot: i64,
    requests: u64,
    refused: u64,
    broken: u64,
    p95: f64,
}

#[derive(Debug, Row, Deserialize)]
struct RequestWholeRow {
    requests: u64,
    answered: u64,
    refused: u64,
    broken: u64,
    latency: Vec<f64>,
}

fn bound(
    reader: &ClickhouseReader,
    sql: &str,
    tenant: TenantId,
    filter: &ServiceFilter,
) -> clickhouse::query::Query {
    let environment = filter.environment.as_deref().unwrap_or_default();
    let placed = Placed::of(&filter.placement, Held::Attributes);
    let query = reader
        .client()
        .query(
            &sql.replace("{counted}", COUNTED)
                .replace("{requested}", REQUESTED)
                .replace("{route}", ROUTE)
                .replace("{method}", METHOD)
                .replace("{broken}", BROKEN)
                .replace("{placed}", &placed.and()),
        )
        .bind(tenant.as_uuid())
        .bind(nanos(filter.window.from_unix_nanos()))
        .bind(nanos(filter.window.to_unix_nanos()))
        .bind(u8::from(filter.project.is_some()))
        .bind(uuid_of(filter.project))
        .bind(environment)
        .bind(environment)
        .bind(environment);
    placed.bind(query)
}

fn read_request(row: RequestRow) -> RequestSummary {
    RequestSummary {
        route: row.route.into(),
        service: row.service.into(),
        method: row.method.into(),
        requests: row.requests,
        answered: row.answered,
        refused: row.refused,
        broken: row.broken,
        p50_nanos: quantile(&row.latency, 0),
        p95_nanos: quantile(&row.latency, 1),
        total_nanos: unsigned(row.total_nanos),
        last_seen_unix_nanos: unsigned(row.last_seen),
    }
}

pub(crate) async fn summaries(
    reader: &ClickhouseReader,
    tenant: TenantId,
    grouping: RequestGrouping,
    filter: &ServiceFilter,
) -> Result<Vec<RequestSummary>, StoreError> {
    let sql = match grouping {
        RequestGrouping::Route => BY_ROUTE,
        RequestGrouping::Service => BY_SERVICE,
        RequestGrouping::Method => BY_METHOD,
    };
    let rows: Vec<RequestRow> = bound(reader, sql, tenant, filter)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))?;
    Ok(rows.into_iter().map(read_request).collect())
}

pub(crate) async fn series(
    reader: &ClickhouseReader,
    tenant: TenantId,
    filter: &ServiceFilter,
) -> Result<RequestSeries, StoreError> {
    let from = filter.window.from_unix_nanos();
    let step = filter.step_nanos();
    let stepped = STEPS
        .replace("{from}", &nanos(from).to_string())
        .replace("{step}", &nanos(step).to_string());
    let steps: Vec<RequestStepRow> = bound(reader, &stepped, tenant, filter)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))?;
    let whole: RequestWholeRow = bound(reader, WHOLE, tenant, filter)
        .fetch_one()
        .await
        .map_err(|error| refused(&error))?;

    Ok(RequestSeries {
        step_nanos: step,
        requests: whole.requests,
        answered: whole.answered,
        refused: whole.refused,
        broken: whole.broken,
        p50_nanos: (whole.requests > 0).then(|| quantile(&whole.latency, 0)),
        p95_nanos: (whole.requests > 0).then(|| quantile(&whole.latency, 1)),
        p99_nanos: (whole.requests > 0).then(|| quantile(&whole.latency, 2)),
        points: steps
            .into_iter()
            .map(|row| RequestPoint {
                start_unix_nanos: from + unsigned(row.slot) * step,
                requests: row.requests,
                refused: row.refused,
                broken: row.broken,
                p95_nanos: Some(nanos_of(row.p95)),
            })
            .collect(),
    })
}
