use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseMigrator, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::{
    CacheGrouping, ProjectId, Scope, ServiceFilter, Span, SpanId, SpanKind, SpanReader, SpanSink,
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
    store: Option<(&'static str, &'static str)>,
    hit: Option<bool>,
}

impl Sent {
    fn asked(seed: u32, name: &'static str, at: u64, took: u64) -> Self {
        Self {
            seed,
            service: "api",
            name,
            at,
            took,
            kind: SpanKind::Client,
            store: Some(("db.system.name", "redis")),
            hit: None,
        }
    }

    fn hit(mut self) -> Self {
        self.hit = Some(true);
        self
    }

    fn missed(mut self) -> Self {
        self.hit = Some(false);
        self
    }

    fn on(mut self, store: &'static str) -> Self {
        self.store = Some(("db.system.name", store));
        self
    }

    fn on_the_older_key(mut self, store: &'static str) -> Self {
        self.store = Some(("db.system", store));
        self
    }

    fn without_a_store(mut self) -> Self {
        self.store = None;
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

    fn span(&self) -> Span {
        let id = self.seed.to_be_bytes();
        let mut trace = [9u8; 16];
        trace[12..].copy_from_slice(&id);
        let mut span = [7u8; 8];
        span[4..].copy_from_slice(&id);
        let mut attributes: BTreeMap<Box<str>, Box<str>> = BTreeMap::new();
        if let Some((key, store)) = self.store {
            attributes.insert(key.into(), store.into());
        }
        if let Some(hit) = self.hit {
            attributes.insert("cache.hit".into(), hit.to_string().into());
        }
        Span {
            trace_id: TraceId::try_from(trace.as_slice()).expect("sixteen bytes"),
            span_id: SpanId::try_from(span.as_slice()).expect("eight bytes"),
            parent: None,
            name: self.name.into(),
            service_name: self.service.into(),
            kind: self.kind,
            status: SpanStatus::Ok,
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
async fn every_operation_is_counted_by_hits_and_misses_and_timed() {
    let Some(client) = prepared("ferristrace_cache_operations").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::asked(1, "GET", SECOND, 2 * MILLI).hit(),
            Sent::asked(2, "GET", 2 * SECOND, 2 * MILLI).hit(),
            Sent::asked(3, "GET", 3 * SECOND, 2 * MILLI).hit(),
            Sent::asked(4, "GET", 4 * SECOND, 2 * MILLI).missed(),
            Sent::asked(5, "SET", 5 * SECOND, MILLI),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .caches(org(MINE), CacheGrouping::Operation, &the_hour())
        .await
        .expect("the operations read");

    let names: Vec<&str> = rows.iter().map(|row| &*row.operation).collect();
    assert_eq!(names, vec!["GET", "SET"], "busiest first");
    let read = &rows[0];
    assert_eq!(&*read.store, "redis");
    assert_eq!(read.calls, 4);
    assert_eq!(read.hits, 3);
    assert_eq!(read.misses, 1);
    assert_eq!(read.hit_rate(), Some(0.75));
    assert_eq!(read.p95_nanos, 2 * MILLI);
    assert_eq!(read.last_seen_unix_nanos, BASE + 4 * SECOND);
}

#[tokio::test]
async fn an_operation_no_instrumentation_judged_has_no_hit_rate() {
    let Some(client) = prepared("ferristrace_cache_unjudged").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::asked(1, "SET", SECOND, MILLI),
            Sent::asked(2, "SET", 2 * SECOND, MILLI),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .caches(org(MINE), CacheGrouping::Operation, &the_hour())
        .await
        .expect("the operations read");

    let written = &rows[0];
    assert_eq!(written.calls, 2, "the call is counted");
    assert_eq!((written.hits, written.misses), (0, 0));
    assert_eq!(
        written.hit_rate(),
        None,
        "no cache.hit means no verdict, not a hit rate of zero"
    );
}

#[tokio::test]
async fn a_span_carrying_cache_hit_is_read_whatever_its_store() {
    let Some(client) = prepared("ferristrace_cache_storeless").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::asked(1, "cache.get", SECOND, MILLI)
            .without_a_store()
            .hit()],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .caches(org(MINE), CacheGrouping::Operation, &the_hour())
        .await
        .expect("the operations read");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].hits, 1);
    assert_eq!(
        &*rows[0].store, "unknown",
        "a cache span naming no store is still a cache span"
    );
}

#[tokio::test]
async fn the_current_store_attribute_is_read_before_the_older_one() {
    let Some(client) = prepared("ferristrace_cache_store_keys").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::asked(1, "GET", SECOND, MILLI).hit(),
            Sent::asked(2, "GET", 2 * SECOND, MILLI)
                .on_the_older_key("memcached")
                .hit(),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .caches(org(MINE), CacheGrouping::Store, &the_hour())
        .await
        .expect("the stores read");

    let mut stores: Vec<&str> = rows.iter().map(|row| &*row.store).collect();
    stores.sort_unstable();
    assert_eq!(
        stores,
        vec!["memcached", "redis"],
        "db.system.name and the older db.system both name a store"
    );
}

#[tokio::test]
async fn a_span_that_is_not_a_client_call_is_no_cache_operation() {
    let Some(client) = prepared("ferristrace_cache_kinds").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::asked(1, "GET", SECOND, MILLI).hit(),
            Sent::asked(2, "GET", 2 * SECOND, MILLI)
                .hit()
                .of_kind(SpanKind::Server),
            Sent::asked(3, "GET", 3 * SECOND, MILLI)
                .hit()
                .of_kind(SpanKind::Internal),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .caches(org(MINE), CacheGrouping::Operation, &the_hour())
        .await
        .expect("the operations read");

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].calls, 1,
        "a cache read is a call the service makes, so a client span alone"
    );
}

#[tokio::test]
async fn a_database_that_is_no_cache_is_left_out() {
    let Some(client) = prepared("ferristrace_cache_other_stores").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::asked(1, "GET", SECOND, MILLI).hit(),
            Sent::asked(2, "SELECT", 2 * SECOND, MILLI).on("postgresql"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .caches(org(MINE), CacheGrouping::Operation, &the_hour())
        .await
        .expect("the operations read");

    let names: Vec<&str> = rows.iter().map(|row| &*row.operation).collect();
    assert_eq!(names, vec!["GET"], "a postgresql statement is a query");
}

#[tokio::test]
async fn grouping_by_store_folds_every_operation_of_a_store_into_one_row() {
    let Some(client) = prepared("ferristrace_cache_by_store").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::asked(1, "GET", SECOND, MILLI).hit(),
            Sent::asked(2, "SET", 2 * SECOND, MILLI),
            Sent::asked(3, "GET", 3 * SECOND, MILLI)
                .on("memcached")
                .missed(),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .caches(org(MINE), CacheGrouping::Store, &the_hour())
        .await
        .expect("the stores read");

    let names: Vec<&str> = rows.iter().map(|row| &*row.operation).collect();
    assert_eq!(names, vec!["redis", "memcached"], "busiest first");
    assert_eq!(rows[0].calls, 2);
    assert_eq!((rows[0].hits, rows[0].misses), (1, 0));
    assert_eq!(rows[1].hit_rate(), Some(0.0));
}

#[tokio::test]
async fn the_same_operation_asked_by_two_services_stays_one_row() {
    let Some(client) = prepared("ferristrace_cache_two_services").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::asked(1, "GET", SECOND, MILLI).hit(),
            Sent::asked(2, "GET", 2 * SECOND, MILLI).by("billing").hit(),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .caches(org(MINE), CacheGrouping::Operation, &the_hour())
        .await
        .expect("the operations read");

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].calls, 2,
        "the cache is shared, so its operations are counted together"
    );
}

#[tokio::test]
async fn a_cache_series_counts_hits_and_misses_of_every_step() {
    let Some(client) = prepared("ferristrace_cache_series").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::asked(1, "GET", 10 * SECOND, 2 * MILLI).hit(),
            Sent::asked(2, "GET", 20 * SECOND, 2 * MILLI).missed(),
            Sent::asked(3, "SET", 5 * MINUTE, MILLI),
        ],
    )
    .await;

    let series = ClickhouseReader::new(client)
        .cache_series(org(MINE), &the_hour())
        .await
        .expect("the series reads");

    assert_eq!(series.calls, 3);
    assert_eq!((series.hits, series.misses), (1, 1));
    assert_eq!(series.hit_rate(), Some(0.5));
    let counted: u64 = series.points.iter().map(|point| point.calls).sum();
    assert_eq!(counted, 3, "every call falls in a step");
    let first = series
        .points
        .iter()
        .find(|point| point.calls > 0)
        .expect("a step holding the first two calls");
    assert_eq!((first.calls, first.hits, first.misses), (2, 1, 1));
    assert_eq!(first.hit_rate(), Some(0.5));
}

#[tokio::test]
async fn a_project_bound_read_only_counts_the_operations_of_that_project() {
    let Some(client) = prepared("ferristrace_cache_project").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(7))),
        &[Sent::asked(1, "GET", SECOND, MILLI).hit()],
    )
    .await;
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(8))),
        &[Sent::asked(2, "SET", 2 * SECOND, MILLI)],
    )
    .await;

    let mut filter = the_hour();
    filter.project = Some(project(7));
    let rows = ClickhouseReader::new(client)
        .caches(org(MINE), CacheGrouping::Operation, &filter)
        .await
        .expect("the operations read");

    let names: Vec<&str> = rows.iter().map(|row| &*row.operation).collect();
    assert_eq!(names, vec!["GET"]);
}
