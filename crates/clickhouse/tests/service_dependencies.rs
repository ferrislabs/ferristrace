use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseMigrator, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::{
    ProjectId, Scope, ServiceFilter, Span, SpanId, SpanKind, SpanReader, SpanSink, SpanStatus,
    TenantId, TraceId, Window,
};
use ferristrace_infra::FixedClock;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const BASE: u64 = 1_700_000_000_000_000_000;
const SECOND: u64 = 1_000_000_000;
const HOUR: u64 = 3_600 * SECOND;
const MINE: u128 = 1;
const THEIRS: u128 = 2;

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

fn the_hour() -> ServiceFilter {
    ServiceFilter::over(Window::new(BASE, BASE + HOUR).expect("an hour"))
}

fn id(trace: u8, span: u8) -> (TraceId, SpanId) {
    (
        TraceId::try_from([trace; 16].as_slice()).expect("sixteen bytes"),
        SpanId::try_from([span; 8].as_slice()).expect("eight bytes"),
    )
}

#[allow(clippy::too_many_arguments)]
fn span(
    trace: u8,
    this: u8,
    parent: Option<u8>,
    service: &str,
    kind: SpanKind,
    status: SpanStatus,
    at: u64,
) -> Span {
    let (trace_id, span_id) = id(trace, this);
    Span {
        trace_id,
        span_id,
        parent: parent.map(|parent| id(trace, parent).1),
        name: "call".into(),
        service_name: service.into(),
        kind,
        status,
        start_unix_nanos: BASE + at,
        end_unix_nanos: BASE + at + SECOND,
        attributes: BTreeMap::new(),
        events: Vec::new(),
    }
}

async fn written(client: &Client, scope: Scope, spans: Vec<Span>) {
    let sink = ClickhouseSink::new(
        client,
        BatchLimits {
            max_rows: 100_000,
            period: Duration::from_secs(60),
        },
        Duration::from_secs(7 * 24 * 60 * 60),
        FixedClock::at(NOW),
    );
    sink.accept(scope, spans).await.expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
}

#[tokio::test]
async fn a_server_span_whose_parent_lives_in_another_service_is_an_edge() {
    let Some(client) = prepared("ferristrace_dependencies_basic").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        vec![
            span(
                1,
                1,
                None,
                "checkout",
                SpanKind::Internal,
                SpanStatus::Ok,
                0,
            ),
            span(
                1,
                2,
                Some(1),
                "billing",
                SpanKind::Server,
                SpanStatus::Ok,
                1,
            ),
            span(
                1,
                3,
                Some(1),
                "billing",
                SpanKind::Server,
                SpanStatus::Error,
                2,
            ),
        ],
    )
    .await;

    let edges = ClickhouseReader::new(client)
        .dependencies(org(MINE), &the_hour())
        .await
        .expect("the dependencies read");

    assert_eq!(edges.len(), 1, "one edge: checkout -> billing");
    let edge = &edges[0];
    assert_eq!((&*edge.caller, &*edge.callee), ("checkout", "billing"));
    assert_eq!((edge.calls, edge.errors), (2, 1));
}

#[tokio::test]
async fn a_call_within_the_same_service_is_not_an_edge() {
    let Some(client) = prepared("ferristrace_dependencies_same_service").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        vec![
            span(
                2,
                1,
                None,
                "checkout",
                SpanKind::Internal,
                SpanStatus::Ok,
                0,
            ),
            span(
                2,
                2,
                Some(1),
                "checkout",
                SpanKind::Server,
                SpanStatus::Ok,
                1,
            ),
        ],
    )
    .await;

    let edges = ClickhouseReader::new(client)
        .dependencies(org(MINE), &the_hour())
        .await
        .expect("the dependencies read");

    assert!(
        edges.is_empty(),
        "a span calling its own service is no edge"
    );
}

#[tokio::test]
async fn the_dependency_graph_is_scoped_to_the_tenant_and_project() {
    let Some(client) = prepared("ferristrace_dependencies_scoped").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        vec![
            span(
                3,
                1,
                None,
                "checkout",
                SpanKind::Internal,
                SpanStatus::Ok,
                0,
            ),
            span(
                3,
                2,
                Some(1),
                "billing",
                SpanKind::Server,
                SpanStatus::Ok,
                1,
            ),
        ],
    )
    .await;
    let mut other_project = Scope::of(org(MINE));
    other_project.project = Some(project(9));
    written(
        &client,
        other_project,
        vec![
            span(
                4,
                1,
                None,
                "checkout",
                SpanKind::Internal,
                SpanStatus::Ok,
                0,
            ),
            span(
                4,
                2,
                Some(1),
                "shipping",
                SpanKind::Server,
                SpanStatus::Ok,
                1,
            ),
        ],
    )
    .await;
    written(
        &client,
        Scope::of(org(THEIRS)),
        vec![
            span(
                5,
                1,
                None,
                "checkout",
                SpanKind::Internal,
                SpanStatus::Ok,
                0,
            ),
            span(5, 2, Some(1), "fraud", SpanKind::Server, SpanStatus::Ok, 1),
        ],
    )
    .await;

    let reader = ClickhouseReader::new(client);
    let everyone = reader
        .dependencies(org(MINE), &the_hour())
        .await
        .expect("the dependencies read");
    assert_eq!(
        everyone
            .iter()
            .map(|edge| &*edge.callee)
            .collect::<Vec<_>>(),
        vec!["billing", "shipping"],
        "both projects of the tenant, neither of the other tenant"
    );

    let mut scoped = the_hour();
    scoped.project = Some(project(9));
    let one_project = reader
        .dependencies(org(MINE), &scoped)
        .await
        .expect("the dependencies read");
    assert_eq!(
        one_project
            .iter()
            .map(|edge| &*edge.callee)
            .collect::<Vec<_>>(),
        vec!["shipping"]
    );
}
