use clickhouse::Row;
use ferristrace_domain::{
    JobGrouping, JobPoint, JobSeries, JobSummary, ServiceFilter, StoreError, TenantId,
};
use serde::Deserialize;

use crate::clickhouse::placement::{Held, Placed};
use crate::clickhouse::read::{
    ClickhouseReader, nanos, nanos_of, quantile, refused, unsigned, uuid_of,
};

const JOBBED: &str = "FROM spans FINAL \
   WHERE tenant_id = ? \
     AND start_time >= fromUnixTimestamp64Nano(?) \
     AND start_time < fromUnixTimestamp64Nano(?) \
     AND (kind = 'consumer' \
       OR attributes['messaging.operation.type'] = 'process' \
       OR attributes['messaging.operation'] = 'process') \
     AND (? = 0 OR project_id = ?) \
     AND (? = '' OR attributes['deployment.environment'] = ? \
       OR attributes['deployment.environment.name'] = ?){placed}";

const QUEUE: &str = "multiIf(attributes['messaging.destination.name'] != '', \
   attributes['messaging.destination.name'], \
   attributes['messaging.destination'] != '', attributes['messaging.destination'], 'unknown')";

const SYSTEM: &str =
    "multiIf(attributes['messaging.system'] != '', attributes['messaging.system'], 'unknown')";

const JOB_COUNTED: &str = "count() AS processed, \
     countIf(status = 'error') AS failed, \
     sum(toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time)) AS total_nanos, \
     quantiles(0.5, 0.95)( \
       toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time) \
     ) AS latency, \
     toUnixTimestamp64Nano(max(start_time)) AS last_seen";

const JOB_BY_JOB: &str = "SELECT name AS job, {queue} AS queue, any({system}) AS system, \
     {counted} {jobbed} \
   GROUP BY job, queue ORDER BY processed DESC, job, queue LIMIT 500";

const JOB_BY_QUEUE: &str = "SELECT {queue} AS job, {queue} AS queue, any({system}) AS system, \
     {counted} {jobbed} \
   GROUP BY job, queue ORDER BY processed DESC, queue LIMIT 500";

const JOB_BY_SYSTEM: &str = "SELECT {system} AS job, any({queue}) AS queue, {system} AS system, \
     {counted} {jobbed} \
   GROUP BY job, system ORDER BY processed DESC, system LIMIT 500";

const JOB_STEPS: &str = "SELECT \
     toInt64(intDiv(toUnixTimestamp64Nano(start_time) - {from}, {step})) AS slot, \
     count() AS processed, \
     countIf(status = 'error') AS failed, \
     quantile(0.95)(toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time)) AS p95 \
   {jobbed} GROUP BY slot ORDER BY slot";

const JOB_WHOLE: &str = "SELECT \
     count() AS processed, \
     countIf(status = 'error') AS failed, \
     quantiles(0.5, 0.95, 0.99)( \
       toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time) \
     ) AS latency \
   {jobbed}";

#[derive(Debug, Row, Deserialize)]
struct JobRow {
    job: String,
    queue: String,
    system: String,
    processed: u64,
    failed: u64,
    total_nanos: i64,
    latency: Vec<f64>,
    last_seen: i64,
}

#[derive(Debug, Row, Deserialize)]
struct JobStepRow {
    slot: i64,
    processed: u64,
    failed: u64,
    p95: f64,
}

#[derive(Debug, Row, Deserialize)]
struct JobWholeRow {
    processed: u64,
    failed: u64,
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
            &sql.replace("{counted}", JOB_COUNTED)
                .replace("{jobbed}", JOBBED)
                .replace("{queue}", QUEUE)
                .replace("{system}", SYSTEM)
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

pub(crate) async fn summaries(
    reader: &ClickhouseReader,
    tenant: TenantId,
    grouping: JobGrouping,
    filter: &ServiceFilter,
) -> Result<Vec<JobSummary>, StoreError> {
    let sql = match grouping {
        JobGrouping::Job => JOB_BY_JOB,
        JobGrouping::Queue => JOB_BY_QUEUE,
        JobGrouping::System => JOB_BY_SYSTEM,
    };
    let rows: Vec<JobRow> = bound(reader, sql, tenant, filter)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))?;
    Ok(rows.into_iter().map(read_job).collect())
}

pub(crate) async fn series(
    reader: &ClickhouseReader,
    tenant: TenantId,
    filter: &ServiceFilter,
) -> Result<JobSeries, StoreError> {
    let from = filter.window.from_unix_nanos();
    let step = filter.step_nanos();
    let stepped = JOB_STEPS
        .replace("{from}", &nanos(from).to_string())
        .replace("{step}", &nanos(step).to_string());
    let steps: Vec<JobStepRow> = bound(reader, &stepped, tenant, filter)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))?;
    let whole: JobWholeRow = bound(reader, JOB_WHOLE, tenant, filter)
        .fetch_one()
        .await
        .map_err(|error| refused(&error))?;

    Ok(JobSeries {
        step_nanos: step,
        processed: whole.processed,
        failed: whole.failed,
        p50_nanos: (whole.processed > 0).then(|| quantile(&whole.latency, 0)),
        p95_nanos: (whole.processed > 0).then(|| quantile(&whole.latency, 1)),
        p99_nanos: (whole.processed > 0).then(|| quantile(&whole.latency, 2)),
        points: steps
            .into_iter()
            .map(|row| JobPoint {
                start_unix_nanos: from + unsigned(row.slot) * step,
                processed: row.processed,
                failed: row.failed,
                p95_nanos: Some(nanos_of(row.p95)),
            })
            .collect(),
    })
}

fn read_job(row: JobRow) -> JobSummary {
    JobSummary {
        job: row.job.into(),
        queue: row.queue.into(),
        system: row.system.into(),
        processed: row.processed,
        failed: row.failed,
        p50_nanos: quantile(&row.latency, 0),
        p95_nanos: quantile(&row.latency, 1),
        total_nanos: unsigned(row.total_nanos),
        last_seen_unix_nanos: unsigned(row.last_seen),
    }
}
