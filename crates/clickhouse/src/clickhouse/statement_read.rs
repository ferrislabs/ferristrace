use clickhouse::Row;
use ferristrace_domain::{
    Fanout, LEAST_FANOUT, ServiceFilter, StatementGrouping, StatementPoint, StatementSeries,
    StatementSummary, StoreError, TenantId,
};
use serde::Deserialize;

use crate::clickhouse::placement::{Held, Placed};
use crate::clickhouse::read::{
    ClickhouseReader, nanos, nanos_of, quantile, refused, unsigned, uuid_of,
};

const SYSTEM: &str = "multiIf(attributes['db.system.name'] != '', attributes['db.system.name'], \
   attributes['db.system'] != '', attributes['db.system'], 'unknown')";

const DATABASE: &str = "multiIf(attributes['db.namespace'] != '', attributes['db.namespace'], \
   attributes['db.name'] != '', attributes['db.name'], 'unknown')";

const OPERATION: &str = "multiIf(attributes['db.operation.name'] != '', \
   attributes['db.operation.name'], attributes['db.operation'] != '', \
   attributes['db.operation'], 'unknown')";

const STATEMENT: &str = "replaceRegexpAll( \
   replaceRegexpAll( \
     multiIf(attributes['db.query.text'] != '', attributes['db.query.text'], \
       attributes['db.statement'] != '', attributes['db.statement'], name), \
     '\\'[^\\']*\\'', '??'), \
   '[0-9]+', '??')";

const QUERIED: &str = "FROM spans FINAL \
   WHERE tenant_id = ? \
     AND start_time >= fromUnixTimestamp64Nano(?) \
     AND start_time < fromUnixTimestamp64Nano(?) \
     AND kind = 'client' \
     AND {system} != 'unknown' \
     AND {system} NOT IN ('redis', 'memcached', 'valkey') \
     AND (? = 0 OR project_id = ?) \
     AND (? = '' OR attributes['deployment.environment'] = ? \
       OR attributes['deployment.environment.name'] = ?){placed}";

const COUNTED: &str = "count() AS calls, \
     countIf(status = 'error') AS errors, \
     sum(toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time)) AS total_nanos, \
     quantiles(0.5, 0.95)( \
       toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time) \
     ) AS latency, \
     toUnixTimestamp64Nano(max(start_time)) AS last_seen";

const BY_STATEMENT: &str = "SELECT {statement} AS statement, {database} AS database, \
     any({system}) AS system, {counted} {queried} \
   GROUP BY statement, database ORDER BY calls DESC, statement, database LIMIT 500";

const BY_DATABASE: &str = "SELECT {database} AS statement, {database} AS database, \
     any({system}) AS system, {counted} {queried} \
   GROUP BY statement, database ORDER BY calls DESC, statement, database LIMIT 500";

const BY_OPERATION: &str = "SELECT {operation} AS statement, {database} AS database, \
     any({system}) AS system, {counted} {queried} \
   GROUP BY statement, database ORDER BY calls DESC, statement, database LIMIT 500";

const STEPS: &str = "SELECT \
     toInt64(intDiv(toUnixTimestamp64Nano(start_time) - {from}, {step})) AS slot, \
     count() AS calls, \
     countIf(status = 'error') AS errors, \
     quantile(0.95)(toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time)) AS p95 \
   {queried} GROUP BY slot ORDER BY slot";

const WHOLE: &str = "SELECT \
     count() AS calls, \
     countIf(status = 'error') AS errors, \
     quantiles(0.5, 0.95, 0.99)( \
       toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time) \
     ) AS latency \
   {queried}";

#[derive(Debug, Row, Deserialize)]
struct StatementRow {
    statement: String,
    database: String,
    system: String,
    calls: u64,
    errors: u64,
    total_nanos: i64,
    latency: Vec<f64>,
    last_seen: i64,
}

#[derive(Debug, Row, Deserialize)]
struct StepRow {
    slot: i64,
    calls: u64,
    errors: u64,
    p95: f64,
}

#[derive(Debug, Row, Deserialize)]
struct WholeRow {
    calls: u64,
    errors: u64,
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
                .replace("{queried}", QUERIED)
                .replace("{system}", SYSTEM)
                .replace("{database}", DATABASE)
                .replace("{operation}", OPERATION)
                .replace("{statement}", STATEMENT)
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

fn read_statement(row: StatementRow) -> StatementSummary {
    StatementSummary {
        statement: row.statement.into(),
        database: row.database.into(),
        system: row.system.into(),
        calls: row.calls,
        errors: row.errors,
        p50_nanos: quantile(&row.latency, 0),
        p95_nanos: quantile(&row.latency, 1),
        total_nanos: unsigned(row.total_nanos),
        last_seen_unix_nanos: unsigned(row.last_seen),
    }
}

pub(crate) async fn summaries(
    reader: &ClickhouseReader,
    tenant: TenantId,
    grouping: StatementGrouping,
    filter: &ServiceFilter,
) -> Result<Vec<StatementSummary>, StoreError> {
    let sql = match grouping {
        StatementGrouping::Statement => BY_STATEMENT,
        StatementGrouping::Database => BY_DATABASE,
        StatementGrouping::Operation => BY_OPERATION,
    };
    let rows: Vec<StatementRow> = bound(reader, sql, tenant, filter)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))?;
    Ok(rows.into_iter().map(read_statement).collect())
}

pub(crate) async fn series(
    reader: &ClickhouseReader,
    tenant: TenantId,
    filter: &ServiceFilter,
) -> Result<StatementSeries, StoreError> {
    let from = filter.window.from_unix_nanos();
    let step = filter.step_nanos();
    let stepped = STEPS
        .replace("{from}", &nanos(from).to_string())
        .replace("{step}", &nanos(step).to_string());
    let steps: Vec<StepRow> = bound(reader, &stepped, tenant, filter)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))?;
    let whole: WholeRow = bound(reader, WHOLE, tenant, filter)
        .fetch_one()
        .await
        .map_err(|error| refused(&error))?;

    Ok(StatementSeries {
        step_nanos: step,
        calls: whole.calls,
        errors: whole.errors,
        p50_nanos: (whole.calls > 0).then(|| quantile(&whole.latency, 0)),
        p95_nanos: (whole.calls > 0).then(|| quantile(&whole.latency, 1)),
        p99_nanos: (whole.calls > 0).then(|| quantile(&whole.latency, 2)),
        points: steps
            .into_iter()
            .map(|row| StatementPoint {
                start_unix_nanos: from + unsigned(row.slot) * step,
                calls: row.calls,
                errors: row.errors,
                p95_nanos: Some(nanos_of(row.p95)),
            })
            .collect(),
    })
}

const FANOUT: &str = "SELECT statement, count() AS traces, max(calls) AS worst, \
     sum(calls) AS total \
   FROM ( \
     SELECT {statement} AS statement, trace_id, count() AS calls \
       {queried} \
     GROUP BY statement, trace_id \
     HAVING calls >= {least} \
   ) \
   GROUP BY statement ORDER BY worst DESC, statement LIMIT 50";

#[derive(Debug, Row, Deserialize)]
struct FanoutRow {
    statement: String,
    traces: u64,
    worst: u64,
    total: u64,
}

pub(crate) async fn fanouts(
    reader: &ClickhouseReader,
    tenant: TenantId,
    filter: &ServiceFilter,
) -> Result<Vec<Fanout>, StoreError> {
    let asked = FANOUT.replace("{least}", &LEAST_FANOUT.to_string());
    let rows: Vec<FanoutRow> = bound(reader, &asked, tenant, filter)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))?;
    Ok(rows
        .into_iter()
        .map(|row| Fanout {
            statement: row.statement.into(),
            traces: row.traces,
            worst: row.worst,
            calls: row.total,
        })
        .collect())
}
