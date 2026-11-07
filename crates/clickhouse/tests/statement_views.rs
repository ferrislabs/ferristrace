use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseMigrator, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::{
    LEAST_FANOUT, ProjectId, Scope, ServiceFilter, Span, SpanId, SpanKind, SpanReader, SpanSink,
    SpanStatus, StatementGrouping, TenantId, TraceId, Window,
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
    trace: Option<u32>,
    service: &'static str,
    name: &'static str,
    at: u64,
    took: u64,
    kind: SpanKind,
    status: SpanStatus,
    system: Option<(&'static str, &'static str)>,
    query_text: Option<&'static str>,
    statement: Option<&'static str>,
    database: Option<(&'static str, &'static str)>,
    operation: Option<(&'static str, &'static str)>,
}

impl Sent {
    fn queried(seed: u32, name: &'static str, at: u64, took: u64) -> Self {
        Self {
            seed,
            trace: None,
            service: "api",
            name,
            at,
            took,
            kind: SpanKind::Client,
            status: SpanStatus::Ok,
            system: Some(("db.system.name", "postgresql")),
            query_text: None,
            statement: None,
            database: None,
            operation: None,
        }
    }

    fn in_trace(mut self, trace: u32) -> Self {
        self.trace = Some(trace);
        self
    }

    fn texted(mut self, text: &'static str) -> Self {
        self.query_text = Some(text);
        self
    }

    fn stated(mut self, text: &'static str) -> Self {
        self.statement = Some(text);
        self
    }

    fn failed(mut self) -> Self {
        self.status = SpanStatus::Error;
        self
    }

    fn on(mut self, system: &'static str) -> Self {
        self.system = Some(("db.system.name", system));
        self
    }

    fn on_the_older_key(mut self, system: &'static str) -> Self {
        self.system = Some(("db.system", system));
        self
    }

    fn without_a_system(mut self) -> Self {
        self.system = None;
        self
    }

    fn in_database(mut self, name: &'static str) -> Self {
        self.database = Some(("db.namespace", name));
        self
    }

    fn in_the_older_database(mut self, name: &'static str) -> Self {
        self.database = Some(("db.name", name));
        self
    }

    fn doing(mut self, op: &'static str) -> Self {
        self.operation = Some(("db.operation.name", op));
        self
    }

    fn doing_the_older_op(mut self, op: &'static str) -> Self {
        self.operation = Some(("db.operation", op));
        self
    }

    fn of_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    fn by(mut self, service: &'static str) -> Self {
        self.service = service;
        self
    }

    fn span(&self) -> Span {
        let id = self.seed.to_be_bytes();
        let mut trace = [9u8; 16];
        trace[12..].copy_from_slice(&self.trace.unwrap_or(self.seed).to_be_bytes());
        let mut span = [7u8; 8];
        span[4..].copy_from_slice(&id);
        let mut attributes: BTreeMap<Box<str>, Box<str>> = BTreeMap::new();
        if let Some((key, system)) = self.system {
            attributes.insert(key.into(), system.into());
        }
        if let Some(text) = self.query_text {
            attributes.insert("db.query.text".into(), text.into());
        }
        if let Some(text) = self.statement {
            attributes.insert("db.statement".into(), text.into());
        }
        if let Some((key, name)) = self.database {
            attributes.insert(key.into(), name.into());
        }
        if let Some((key, name)) = self.operation {
            attributes.insert(key.into(), name.into());
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
async fn every_statement_is_counted_grouped_and_timed() {
    let Some(client) = prepared("ferristrace_statement_basic").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::queried(1, "query", SECOND, 2 * MILLI)
                .texted("SELECT * FROM users WHERE id = 42 AND name = 'bob'"),
            Sent::queried(2, "query", 2 * SECOND, 2 * MILLI)
                .texted("SELECT * FROM users WHERE id = 7 AND name = 'alice'"),
            Sent::queried(3, "query", 3 * SECOND, 2 * MILLI)
                .texted("SELECT * FROM users WHERE id = 99 AND name = 'carl'")
                .failed(),
            Sent::queried(4, "query", 4 * SECOND, MILLI).texted("SET x = 1"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Statement, &the_hour())
        .await
        .expect("the statements read");

    assert_eq!(rows.len(), 2, "two distinct normalised statements");
    let busiest = &rows[0];
    assert_eq!(
        &*busiest.statement, "SELECT * FROM users WHERE id = ? AND name = ?",
        "numbers and quoted strings fold together"
    );
    assert_eq!(&*busiest.database, "unknown");
    assert_eq!(&*busiest.system, "postgresql");
    assert_eq!(busiest.calls, 3);
    assert_eq!(busiest.errors, 1);
    assert_eq!(busiest.total_nanos, 6 * MILLI);
    assert_eq!(busiest.p50_nanos, 2 * MILLI);
    assert_eq!(busiest.p95_nanos, 2 * MILLI);
    assert_eq!(busiest.last_seen_unix_nanos, BASE + 3 * SECOND);
}

#[tokio::test]
async fn the_query_text_attribute_is_read_before_the_statement_and_the_name() {
    let Some(client) = prepared("ferristrace_statement_precedence").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::queried(1, "ignored name a", SECOND, MILLI)
                .texted("SELECT A")
                .stated("WRONG A"),
            Sent::queried(2, "ignored name b", 2 * SECOND, MILLI).stated("SELECT B"),
            Sent::queried(3, "SELECT C", 3 * SECOND, MILLI),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Statement, &the_hour())
        .await
        .expect("the statements read");

    let mut statements: Vec<&str> = rows.iter().map(|row| &*row.statement).collect();
    statements.sort_unstable();
    assert_eq!(
        statements,
        vec!["SELECT A", "SELECT B", "SELECT C"],
        "db.query.text wins over db.statement, which wins over the span name"
    );
}

#[tokio::test]
async fn the_database_falls_back_through_namespace_then_name_then_unknown() {
    let Some(client) = prepared("ferristrace_statement_database_fallback").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::queried(1, "query", SECOND, MILLI)
                .texted("SELECT 1")
                .in_database("shop"),
            Sent::queried(2, "query", 2 * SECOND, MILLI)
                .texted("SELECT 1")
                .in_the_older_database("legacy"),
            Sent::queried(3, "query", 3 * SECOND, MILLI).texted("SELECT 1"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Statement, &the_hour())
        .await
        .expect("the statements read");

    let mut databases: Vec<&str> = rows.iter().map(|row| &*row.database).collect();
    databases.sort_unstable();
    assert_eq!(
        databases,
        vec!["legacy", "shop", "unknown"],
        "db.namespace wins over db.name, with unknown when neither is set"
    );
}

#[tokio::test]
async fn the_operation_falls_back_through_operation_name_then_operation_then_unknown() {
    let Some(client) = prepared("ferristrace_statement_operation_fallback").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::queried(1, "query", SECOND, MILLI).doing("SELECT"),
            Sent::queried(2, "query", 2 * SECOND, MILLI).doing_the_older_op("INSERT"),
            Sent::queried(3, "query", 3 * SECOND, MILLI),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Operation, &the_hour())
        .await
        .expect("the operations read");

    let mut operations: Vec<&str> = rows.iter().map(|row| &*row.statement).collect();
    operations.sort_unstable();
    assert_eq!(
        operations,
        vec!["INSERT", "SELECT", "unknown"],
        "db.operation.name wins over db.operation, with unknown when neither is set"
    );
}

#[tokio::test]
async fn a_span_that_is_not_a_client_call_is_no_statement() {
    let Some(client) = prepared("ferristrace_statement_kinds").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::queried(1, "query", SECOND, MILLI).texted("SELECT 1"),
            Sent::queried(2, "query", 2 * SECOND, MILLI)
                .texted("SELECT 1")
                .of_kind(SpanKind::Server),
            Sent::queried(3, "query", 3 * SECOND, MILLI)
                .texted("SELECT 1")
                .of_kind(SpanKind::Internal),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Statement, &the_hour())
        .await
        .expect("the statements read");

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].calls, 1,
        "a database call is a client span the service makes, so a client span alone"
    );
}

#[tokio::test]
async fn a_system_we_do_not_recognise_is_excluded() {
    let Some(client) = prepared("ferristrace_statement_unknown_system").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::queried(1, "rpc", SECOND, MILLI)
            .texted("SELECT 1")
            .without_a_system()],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Statement, &the_hour())
        .await
        .expect("the statements read");

    assert_eq!(
        rows.len(),
        0,
        "a client span naming no database system is not a database statement"
    );
}

#[tokio::test]
async fn a_cache_store_system_is_excluded_even_as_a_client_call() {
    let Some(client) = prepared("ferristrace_statement_cache_stores").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::queried(1, "GET", SECOND, MILLI).on("redis"),
            Sent::queried(2, "GET", 2 * SECOND, MILLI).on_the_older_key("memcached"),
            Sent::queried(3, "GET", 3 * SECOND, MILLI).on("valkey"),
            Sent::queried(4, "query", 4 * SECOND, MILLI).texted("SELECT 1"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Statement, &the_hour())
        .await
        .expect("the statements read");

    assert_eq!(
        rows.len(),
        1,
        "redis, memcached and valkey are the cache page's own statements"
    );
    assert_eq!(&*rows[0].statement, "SELECT ?");
}

#[tokio::test]
async fn grouping_by_database_folds_every_statement_of_a_database_into_one_row() {
    let Some(client) = prepared("ferristrace_statement_by_database").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::queried(1, "query", SECOND, MILLI)
                .texted("SELECT 1")
                .in_database("shop"),
            Sent::queried(2, "query", 2 * SECOND, MILLI)
                .texted("SELECT 2")
                .in_database("shop"),
            Sent::queried(3, "query", 3 * SECOND, MILLI)
                .texted("SELECT 3")
                .in_database("billing"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Database, &the_hour())
        .await
        .expect("the databases read");

    let mut names: Vec<&str> = rows.iter().map(|row| &*row.statement).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["billing", "shop"]);
    let shop = rows
        .iter()
        .find(|row| &*row.statement == "shop")
        .expect("the two shop statements fold into one row");
    assert_eq!(shop.calls, 2);
    assert_eq!(&*shop.database, "shop");
}

#[tokio::test]
async fn grouping_by_operation_keeps_the_database_breakdown() {
    let Some(client) = prepared("ferristrace_statement_by_operation").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::queried(1, "query", SECOND, MILLI)
                .doing("SELECT")
                .in_database("shop"),
            Sent::queried(2, "query", 2 * SECOND, MILLI)
                .doing("SELECT")
                .in_database("billing"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Operation, &the_hour())
        .await
        .expect("the operations read");

    assert_eq!(
        rows.len(),
        2,
        "the same operation on two databases stays two rows"
    );
    let mut databases: Vec<&str> = rows.iter().map(|row| &*row.database).collect();
    databases.sort_unstable();
    assert_eq!(databases, vec!["billing", "shop"]);
    for row in &rows {
        assert_eq!(&*row.statement, "SELECT");
        assert_eq!(row.calls, 1);
    }
}

#[tokio::test]
async fn a_statement_series_counts_calls_and_errors_of_every_step() {
    let Some(client) = prepared("ferristrace_statement_series").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::queried(1, "query", 10 * SECOND, 2 * MILLI).texted("SELECT 1"),
            Sent::queried(2, "query", 20 * SECOND, 2 * MILLI)
                .texted("SELECT 1")
                .failed(),
            Sent::queried(3, "query", 5 * MINUTE, MILLI).texted("SELECT 2"),
        ],
    )
    .await;

    let series = ClickhouseReader::new(client)
        .statement_series(org(MINE), &the_hour())
        .await
        .expect("the series reads");

    assert_eq!(series.calls, 3);
    assert_eq!(series.errors, 1);
    assert_eq!(series.error_rate(), Some(1.0 / 3.0));
    let counted: u64 = series.points.iter().map(|point| point.calls).sum();
    assert_eq!(counted, 3, "every call falls in a step");
    let first = series
        .points
        .iter()
        .find(|point| point.calls > 0)
        .expect("a step holding the first two calls");
    assert_eq!((first.calls, first.errors), (2, 1));
}

#[tokio::test]
async fn a_project_bound_read_only_counts_the_statements_of_that_project() {
    let Some(client) = prepared("ferristrace_statement_project").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(7))),
        &[Sent::queried(1, "query", SECOND, MILLI).texted("SELECT 1")],
    )
    .await;
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(8))),
        &[Sent::queried(2, "query", 2 * SECOND, MILLI)
            .texted("SELECT 2")
            .by("billing")],
    )
    .await;

    let mut filter = the_hour();
    filter.project = Some(project(7));
    let rows = ClickhouseReader::new(client)
        .statements(org(MINE), StatementGrouping::Statement, &filter)
        .await
        .expect("the statements read");

    let names: Vec<&str> = rows.iter().map(|row| &*row.statement).collect();
    assert_eq!(names, vec!["SELECT ?"]);
}

fn repeats() -> u32 {
    u32::try_from(LEAST_FANOUT).expect("the fanout threshold fits a u32")
}

#[tokio::test]
async fn a_statement_repeated_inside_one_trace_is_a_possible_fanout() {
    let Some(client) = prepared("ferristrace_statement_fanout").await else {
        return;
    };
    let mut sent: Vec<Sent> = (0..repeats())
        .map(|i| {
            Sent::queried(100 + i, "query", SECOND + u64::from(i) * MILLI, MILLI)
                .in_trace(1)
                .texted("SELECT price FROM products WHERE sku = 1")
        })
        .collect();
    sent.extend((0..3).map(|i| {
        Sent::queried(200 + i, "query", 2 * SECOND, MILLI)
            .in_trace(2)
            .texted("SELECT * FROM orders WHERE id = 1")
    }));
    written(&client, Scope::of(org(MINE)), &sent).await;

    let found = ClickhouseReader::new(client)
        .statement_fanouts(org(MINE), &the_hour())
        .await
        .expect("the fanouts read");

    assert_eq!(found.len(), 1, "only the repeated statement fans out");
    let worst = &found[0];
    assert_eq!(
        &*worst.statement,
        "SELECT price FROM products WHERE sku = ?"
    );
    assert_eq!(worst.worst, LEAST_FANOUT);
    assert_eq!(worst.traces, 1);
    assert_eq!(worst.calls, LEAST_FANOUT);
}

#[tokio::test]
async fn a_statement_spread_over_many_traces_is_no_fanout() {
    let Some(client) = prepared("ferristrace_statement_spread").await else {
        return;
    };
    let sent: Vec<Sent> = (0..repeats() + 10)
        .map(|i| {
            Sent::queried(300 + i, "query", SECOND + u64::from(i) * MILLI, MILLI)
                .texted("SELECT price FROM products WHERE sku = 1")
        })
        .collect();
    written(&client, Scope::of(org(MINE)), &sent).await;

    let found = ClickhouseReader::new(client)
        .statement_fanouts(org(MINE), &the_hour())
        .await
        .expect("the fanouts read");

    assert!(
        found.is_empty(),
        "one call per trace is the opposite of an N+1"
    );
}
