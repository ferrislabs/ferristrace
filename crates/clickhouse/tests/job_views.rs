use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseMigrator, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::{
    JobGrouping, ProjectId, Scope, ServiceFilter, Span, SpanId, SpanKind, SpanReader, SpanSink,
    SpanStatus, TenantId, TraceId, Window,
};
use ferristrace_infra::FixedClock;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const BASE: u64 = 1_700_000_000_000_000_000;
const SECOND: u64 = 1_000_000_000;
const MILLI: u64 = 1_000_000;
const MINUTE: u64 = 60 * SECOND;
const HOUR: u64 = 60 * MINUTE;
const MINE: u128 = 1;

fn connected(url: &str) -> Client {
    let mut client = Client::default().with_url(url);
    if let Ok(user) = std::env::var("CLICKHOUSE_USER") {
        client = client.with_user(user);
    }
    if let Ok(password) = std::env::var("CLICKHOUSE_PASSWORD") {
        client = client.with_password(password);
    }
    client
}

async fn prepared(name: &str) -> Option<Client> {
    let url = backend::clickhouse_url()?;
    let root = connected(&url);
    root.query(&format!("DROP DATABASE IF EXISTS {name}"))
        .execute()
        .await
        .expect("dropping a leftover database");
    root.query(&format!("CREATE DATABASE {name}"))
        .execute()
        .await
        .expect("creating the test database");
    let client = connected(&url).with_database(name);
    ClickhouseMigrator::new(client.clone())
        .up()
        .await
        .expect("the migrations apply");
    Some(client)
}

fn org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

struct Sent {
    seed: u32,
    service: &'static str,
    name: &'static str,
    at: u64,
    took: u64,
    kind: SpanKind,
    status: SpanStatus,
    queue: Option<(&'static str, &'static str)>,
    system: Option<&'static str>,
    process_operation: Option<&'static str>,
}

impl Sent {
    fn worked(seed: u32, name: &'static str, at: u64, took: u64) -> Self {
        Self {
            seed,
            service: "worker",
            name,
            at,
            took,
            kind: SpanKind::Consumer,
            status: SpanStatus::Ok,
            queue: Some(("messaging.destination.name", "orders")),
            system: Some("kafka"),
            process_operation: None,
        }
    }

    fn failed(mut self) -> Self {
        self.status = SpanStatus::Error;
        self
    }

    fn on_queue(mut self, queue: &'static str) -> Self {
        self.queue = Some(("messaging.destination.name", queue));
        self
    }

    fn on_the_older_queue_key(mut self, queue: &'static str) -> Self {
        self.queue = Some(("messaging.destination", queue));
        self
    }

    fn without_a_queue(mut self) -> Self {
        self.queue = None;
        self
    }

    fn on_system(mut self, system: &'static str) -> Self {
        self.system = Some(system);
        self
    }

    fn by(mut self, service: &'static str) -> Self {
        self.service = service;
        self
    }

    fn of_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    fn processing(mut self) -> Self {
        self.kind = SpanKind::Internal;
        self.process_operation = Some("messaging.operation.type");
        self
    }

    fn via_the_older_operation_attribute(mut self) -> Self {
        self.process_operation = Some("messaging.operation");
        self
    }

    fn span(&self) -> Span {
        let id = self.seed.to_be_bytes();
        let mut trace = [9u8; 16];
        trace[12..].copy_from_slice(&id);
        let mut span = [7u8; 8];
        span[4..].copy_from_slice(&id);
        let mut attributes: BTreeMap<Box<str>, Box<str>> = BTreeMap::new();
        if let Some((key, queue)) = self.queue {
            attributes.insert(key.into(), queue.into());
        }
        if let Some(system) = self.system {
            attributes.insert("messaging.system".into(), system.into());
        }
        if let Some(key) = self.process_operation {
            attributes.insert(key.into(), "process".into());
        }
        Span {
            trace_id: TraceId::try_from(trace.as_slice()).expect("sixteen bytes"),
            span_id: SpanId::try_from(span.as_slice()).expect("eight bytes"),
            parent: None,
            name: self.name.into(),
            service_name: self.service.into(),
            kind: self.kind,
            status: self.status,
            start_unix_nanos: BASE + self.at,
            end_unix_nanos: BASE + self.at + self.took,
            attributes,
            events: Vec::new(),
        }
    }
}

async fn written(client: &Client, scope: Scope, sent: &[Sent]) {
    let sink = ClickhouseSink::new(
        client,
        BatchLimits {
            max_rows: 100_000,
            period: Duration::from_secs(60),
        },
        Duration::from_secs(7 * 24 * 60 * 60),
        FixedClock::at(NOW),
    );
    sink.accept(scope, sent.iter().map(Sent::span).collect())
        .await
        .expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
}

fn the_hour() -> ServiceFilter {
    ServiceFilter::over(Window::new(BASE, BASE + HOUR).expect("an hour"))
}

#[tokio::test]
async fn every_job_is_counted_by_processed_and_failed_and_timed() {
    let Some(client) = prepared("ferristrace_job_basics").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::worked(1, "send-email", SECOND, 2 * MILLI),
            Sent::worked(2, "send-email", 2 * SECOND, 2 * MILLI),
            Sent::worked(3, "send-email", 3 * SECOND, 2 * MILLI).failed(),
            Sent::worked(4, "generate-report", 4 * SECOND, MILLI),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::Job, &the_hour())
        .await
        .expect("the jobs read");

    let names: Vec<&str> = rows.iter().map(|row| &*row.job).collect();
    assert_eq!(
        names,
        vec!["send-email", "generate-report"],
        "busiest first"
    );
    let read = &rows[0];
    assert_eq!(&*read.queue, "orders");
    assert_eq!(&*read.system, "kafka");
    assert_eq!(read.processed, 3);
    assert_eq!(read.failed, 1);
    assert_eq!(read.failure_rate(), Some(1.0 / 3.0));
    assert_eq!(read.p95_nanos, 2 * MILLI);
    assert_eq!(read.total_nanos, 6 * MILLI);
    assert_eq!(read.last_seen_unix_nanos, BASE + 3 * SECOND);
}

#[tokio::test]
async fn a_span_naming_no_queue_is_grouped_under_unknown() {
    let Some(client) = prepared("ferristrace_job_queueless").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::worked(1, "send-email", SECOND, MILLI).without_a_queue()],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::Job, &the_hour())
        .await
        .expect("the jobs read");

    assert_eq!(rows.len(), 1);
    assert_eq!(&*rows[0].queue, "unknown");
}

#[tokio::test]
async fn the_current_queue_attribute_is_read_before_the_older_one() {
    let Some(client) = prepared("ferristrace_job_queue_keys").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::worked(1, "send-email", SECOND, MILLI),
            Sent::worked(2, "send-email", 2 * SECOND, MILLI).on_the_older_queue_key("legacy"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::Queue, &the_hour())
        .await
        .expect("the queues read");

    let mut queues: Vec<&str> = rows.iter().map(|row| &*row.job).collect();
    queues.sort_unstable();
    assert_eq!(
        queues,
        vec!["legacy", "orders"],
        "messaging.destination.name and the older messaging.destination both name a queue"
    );
}

#[tokio::test]
async fn a_span_that_is_neither_a_consumer_nor_a_process_operation_is_no_job() {
    let Some(client) = prepared("ferristrace_job_kinds").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::worked(1, "send-email", SECOND, MILLI),
            Sent::worked(2, "GET /orders", 2 * SECOND, MILLI).of_kind(SpanKind::Server),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::Job, &the_hour())
        .await
        .expect("the jobs read");

    assert_eq!(rows.len(), 1);
    assert_eq!(&*rows[0].job, "send-email");
}

#[tokio::test]
async fn a_process_operation_span_is_a_job_even_when_its_kind_is_not_consumer() {
    let Some(client) = prepared("ferristrace_job_process_operation").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::worked(1, "cron-cleanup", SECOND, MILLI).processing()],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::Job, &the_hour())
        .await
        .expect("the jobs read");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].processed, 1);
}

#[tokio::test]
async fn the_older_messaging_operation_attribute_is_also_read() {
    let Some(client) = prepared("ferristrace_job_legacy_operation").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::worked(1, "cron-cleanup", SECOND, MILLI)
            .processing()
            .via_the_older_operation_attribute()],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::Job, &the_hour())
        .await
        .expect("the jobs read");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].processed, 1);
}

#[tokio::test]
async fn grouping_by_queue_folds_every_job_of_a_queue_into_one_row() {
    let Some(client) = prepared("ferristrace_job_by_queue").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::worked(1, "send-email", SECOND, MILLI),
            Sent::worked(2, "send-sms", 2 * SECOND, MILLI),
            Sent::worked(3, "generate-report", 3 * SECOND, MILLI)
                .on_queue("reports")
                .failed(),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::Queue, &the_hour())
        .await
        .expect("the queues read");

    let names: Vec<&str> = rows.iter().map(|row| &*row.job).collect();
    assert_eq!(names, vec!["orders", "reports"], "busiest first");
    assert_eq!(rows[0].processed, 2);
    assert_eq!((rows[1].processed, rows[1].failed), (1, 1));
}

#[tokio::test]
async fn grouping_by_system_folds_every_job_of_a_system_into_one_row() {
    let Some(client) = prepared("ferristrace_job_by_system").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::worked(1, "send-email", SECOND, MILLI),
            Sent::worked(2, "send-sms", 2 * SECOND, MILLI),
            Sent::worked(3, "generate-report", 3 * SECOND, MILLI).on_system("rabbitmq"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::System, &the_hour())
        .await
        .expect("the systems read");

    let mut systems: Vec<&str> = rows.iter().map(|row| &*row.job).collect();
    systems.sort_unstable();
    assert_eq!(systems, vec!["kafka", "rabbitmq"]);
}

#[tokio::test]
async fn the_same_job_run_by_two_services_stays_one_row() {
    let Some(client) = prepared("ferristrace_job_two_services").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::worked(1, "send-email", SECOND, MILLI),
            Sent::worked(2, "send-email", 2 * SECOND, MILLI).by("notifications"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::Job, &the_hour())
        .await
        .expect("the jobs read");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].processed, 2);
}

#[tokio::test]
async fn a_job_series_counts_processed_and_failed_of_every_step() {
    let Some(client) = prepared("ferristrace_job_series").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::worked(1, "send-email", 10 * SECOND, 2 * MILLI),
            Sent::worked(2, "send-email", 20 * SECOND, 2 * MILLI).failed(),
            Sent::worked(3, "generate-report", 5 * MINUTE, MILLI),
        ],
    )
    .await;

    let series = ClickhouseReader::new(client)
        .job_series(org(MINE), &the_hour())
        .await
        .expect("the series reads");

    assert_eq!(series.processed, 3);
    assert_eq!(series.failed, 1);
    assert_eq!(series.failure_rate(), Some(1.0 / 3.0));
    let counted: u64 = series.points.iter().map(|point| point.processed).sum();
    assert_eq!(counted, 3, "every job falls in a step");
    let first = series
        .points
        .iter()
        .find(|point| point.processed > 0)
        .expect("a step holding the first two runs");
    assert_eq!((first.processed, first.failed), (2, 1));
}

#[tokio::test]
async fn a_project_bound_read_only_counts_the_jobs_of_that_project() {
    let Some(client) = prepared("ferristrace_job_project").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(7))),
        &[Sent::worked(1, "send-email", SECOND, MILLI)],
    )
    .await;
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(8))),
        &[Sent::worked(2, "generate-report", 2 * SECOND, MILLI)],
    )
    .await;

    let mut filter = the_hour();
    filter.project = Some(project(7));
    let rows = ClickhouseReader::new(client)
        .jobs(org(MINE), JobGrouping::Job, &filter)
        .await
        .expect("the jobs read");

    let names: Vec<&str> = rows.iter().map(|row| &*row.job).collect();
    assert_eq!(names, vec!["send-email"]);
}
