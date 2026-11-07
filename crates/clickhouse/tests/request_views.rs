use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseMigrator, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::{
    ProjectId, RequestGrouping, Scope, ServiceFilter, Span, SpanId, SpanKind, SpanReader, SpanSink,
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
    method: Option<(&'static str, &'static str)>,
    route: Option<&'static str>,
    code: Option<u16>,
    status: SpanStatus,
}

impl Sent {
    fn served(seed: u32, name: &'static str, at: u64, took: u64) -> Self {
        Self {
            seed,
            service: "api",
            name,
            at,
            took,
            kind: SpanKind::Server,
            method: Some(("http.request.method", "GET")),
            route: None,
            code: Some(200),
            status: SpanStatus::Ok,
        }
    }

    fn routed(mut self, route: &'static str) -> Self {
        self.route = Some(route);
        self
    }

    fn coded(mut self, code: u16) -> Self {
        self.code = Some(code);
        self
    }

    fn uncoded(mut self) -> Self {
        self.code = None;
        self
    }

    fn failed(mut self) -> Self {
        self.code = None;
        self.status = SpanStatus::Error;
        self
    }

    fn verbed(mut self, verb: &'static str) -> Self {
        self.method = Some(("http.request.method", verb));
        self
    }

    fn on_the_older_key(mut self, verb: &'static str) -> Self {
        self.method = Some(("http.method", verb));
        self
    }

    fn without_a_method(mut self) -> Self {
        self.method = None;
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
        if let Some((key, verb)) = self.method {
            attributes.insert(key.into(), verb.into());
        }
        if let Some(route) = self.route {
            attributes.insert("http.route".into(), route.into());
        }
        if let Some(code) = self.code {
            attributes.insert("http.status_code".into(), code.to_string().into());
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
async fn every_request_is_counted_bucketed_and_timed() {
    let Some(client) = prepared("ferristrace_request_basic").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::served(1, "x", SECOND, 2 * MILLI).routed("/orders"),
            Sent::served(2, "x", 2 * SECOND, 2 * MILLI).routed("/orders"),
            Sent::served(3, "x", 3 * SECOND, 2 * MILLI)
                .routed("/orders")
                .coded(404),
            Sent::served(4, "x", 4 * SECOND, 2 * MILLI)
                .routed("/orders")
                .coded(500),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Route, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(rows.len(), 1);
    let read = &rows[0];
    assert_eq!(&*read.route, "/orders");
    assert_eq!(&*read.service, "api");
    assert_eq!(&*read.method, "GET");
    assert_eq!(read.requests, 4);
    assert_eq!(read.answered, 2);
    assert_eq!(read.refused, 1);
    assert_eq!(read.broken, 1);
    assert_eq!(read.p95_nanos, 2 * MILLI);
    assert_eq!(read.total_nanos, 8 * MILLI);
    assert_eq!(read.last_seen_unix_nanos, BASE + 4 * SECOND);
}

#[tokio::test]
async fn a_route_with_no_http_route_falls_back_to_the_span_name() {
    let Some(client) = prepared("ferristrace_request_route_fallback").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::served(1, "GET /legacy", SECOND, MILLI)],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Route, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(rows.len(), 1);
    assert_eq!(&*rows[0].route, "GET /legacy");
}

#[tokio::test]
async fn the_older_http_method_key_still_counts() {
    let Some(client) = prepared("ferristrace_request_method_keys").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::served(1, "x", SECOND, MILLI).on_the_older_key("GET")],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Route, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(rows.len(), 1);
    assert_eq!(&*rows[0].method, "GET");
}

#[tokio::test]
async fn a_span_carrying_only_a_status_code_is_still_a_request() {
    let Some(client) = prepared("ferristrace_request_code_only").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::served(1, "x", SECOND, MILLI).without_a_method()],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Route, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(
        rows.len(),
        1,
        "a status code alone still names an HTTP server span"
    );
    assert_eq!(&*rows[0].method, "unknown");
}

#[tokio::test]
async fn a_span_naming_neither_a_method_nor_a_status_code_is_no_request() {
    let Some(client) = prepared("ferristrace_request_no_marker").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::served(1, "x", SECOND, MILLI)
            .without_a_method()
            .uncoded()],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Route, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(
        rows.len(),
        0,
        "no instrumentation marker means this is not read as a request"
    );
}

#[tokio::test]
async fn a_span_that_is_not_a_server_span_is_no_request() {
    let Some(client) = prepared("ferristrace_request_kinds").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::served(1, "x", SECOND, MILLI),
            Sent::served(2, "x", 2 * SECOND, MILLI).of_kind(SpanKind::Client),
            Sent::served(3, "x", 3 * SECOND, MILLI).of_kind(SpanKind::Internal),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Route, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].requests, 1,
        "a request is a server span, so only the server span counts"
    );
}

#[tokio::test]
async fn an_errored_span_with_no_status_code_is_counted_as_broken() {
    let Some(client) = prepared("ferristrace_request_broken_fallback").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[Sent::served(1, "x", SECOND, MILLI).failed()],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Route, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].broken, 1,
        "no status code but an error status still counts as broken"
    );
    assert_eq!(rows[0].answered, 0);
    assert_eq!(rows[0].refused, 0);
}

#[tokio::test]
async fn grouping_by_route_keeps_different_methods_of_the_same_route_apart() {
    let Some(client) = prepared("ferristrace_request_route_methods").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::served(1, "x", SECOND, MILLI).routed("/orders"),
            Sent::served(2, "x", 2 * SECOND, MILLI)
                .routed("/orders")
                .verbed("POST"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Route, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(
        rows.len(),
        2,
        "a GET and a POST on the same route are different rows"
    );
    let mut methods: Vec<&str> = rows.iter().map(|row| &*row.method).collect();
    methods.sort_unstable();
    assert_eq!(methods, vec!["GET", "POST"]);
}

#[tokio::test]
async fn grouping_by_service_folds_every_route_of_a_service_into_one_row() {
    let Some(client) = prepared("ferristrace_request_by_service").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::served(1, "x", SECOND, MILLI).routed("/orders"),
            Sent::served(2, "x", 2 * SECOND, MILLI)
                .routed("/cart")
                .verbed("POST"),
            Sent::served(3, "x", 3 * SECOND, MILLI)
                .routed("/orders")
                .by("billing"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Service, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(rows.len(), 2, "one row per service");
    let api = rows
        .iter()
        .find(|row| &*row.route == "api")
        .expect("the api service");
    assert_eq!(api.requests, 2);
    assert_eq!(&*api.method, "all");
}

#[tokio::test]
async fn grouping_by_method_folds_every_route_and_service_of_a_verb_into_one_row() {
    let Some(client) = prepared("ferristrace_request_by_method").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::served(1, "x", SECOND, MILLI).routed("/orders"),
            Sent::served(2, "x", 2 * SECOND, MILLI)
                .routed("/cart")
                .by("billing"),
            Sent::served(3, "x", 3 * SECOND, MILLI)
                .routed("/cart")
                .verbed("POST"),
        ],
    )
    .await;

    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Method, &the_hour())
        .await
        .expect("the requests read");

    assert_eq!(rows.len(), 2, "one row per method");
    let get = rows
        .iter()
        .find(|row| &*row.route == "GET")
        .expect("the GET method");
    assert_eq!(get.requests, 2);
    assert_eq!(&*get.service, "all");
}

#[tokio::test]
async fn a_project_bound_read_only_counts_the_requests_of_that_project() {
    let Some(client) = prepared("ferristrace_request_project").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(7))),
        &[Sent::served(1, "x", SECOND, MILLI).routed("/orders")],
    )
    .await;
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(8))),
        &[Sent::served(2, "x", 2 * SECOND, MILLI).routed("/cart")],
    )
    .await;

    let mut filter = the_hour();
    filter.project = Some(project(7));
    let rows = ClickhouseReader::new(client)
        .requests(org(MINE), RequestGrouping::Route, &filter)
        .await
        .expect("the requests read");

    let routes: Vec<&str> = rows.iter().map(|row| &*row.route).collect();
    assert_eq!(routes, vec!["/orders"]);
}

#[tokio::test]
async fn a_request_series_buckets_requests_into_steps_and_totals_the_whole_window() {
    let Some(client) = prepared("ferristrace_request_series").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::served(1, "x", 10 * SECOND, 2 * MILLI).routed("/orders"),
            Sent::served(2, "x", 20 * SECOND, 2 * MILLI)
                .routed("/orders")
                .coded(404),
            Sent::served(3, "x", 5 * MINUTE, MILLI)
                .routed("/cart")
                .coded(500),
        ],
    )
    .await;

    let series = ClickhouseReader::new(client)
        .request_series(org(MINE), &the_hour())
        .await
        .expect("the series reads");

    assert_eq!(series.requests, 3);
    assert_eq!(series.answered, 1);
    assert_eq!(series.refused, 1);
    assert_eq!(series.broken, 1);
    let counted: u64 = series.points.iter().map(|point| point.requests).sum();
    assert_eq!(counted, 3, "every request falls in a step");
    let first = series
        .points
        .iter()
        .find(|point| point.requests > 0)
        .expect("a step holding the first two requests");
    assert_eq!((first.requests, first.refused, first.broken), (2, 1, 0));
}
