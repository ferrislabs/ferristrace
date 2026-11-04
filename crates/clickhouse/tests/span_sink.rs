use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{
    BatchLimits, ClickhouseMigrator, ClickhouseReader, ClickhouseSink, SpanStoreSettings,
    open_span_store,
};
use ferristrace_domain::{
    ProjectId, RetentionDays, Scope, ServiceFilter, Span, SpanEvent, SpanId, SpanKind, SpanReader,
    SpanSink, SpanStatus, TenantId, TraceFilter, TraceId, Window,
};
use ferristrace_infra::{FixedClock, MemorySink};
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;

pub fn a_span(seed: u8, service: &str) -> Span {
    Span {
        trace_id: TraceId::try_from([seed; 16].as_slice()).expect("sixteen bytes"),
        span_id: SpanId::try_from([seed; 8].as_slice()).expect("eight bytes"),
        parent: None,
        name: "work".into(),
        service_name: service.into(),
        kind: SpanKind::Server,
        status: SpanStatus::Ok,
        start_unix_nanos: 1_700_000_000_000_000_000,
        end_unix_nanos: 1_700_000_000_500_000_000,
        attributes: BTreeMap::from([("http.status_code".into(), "200".into())]),
        events: Vec::new(),
    }
}

async fn accepts_batches_from_several_tenants<S: SpanSink>(sink: &S) {
    let first = TenantId::new(Uuid::from_u128(1));
    let second = TenantId::new(Uuid::from_u128(2));

    sink.accept(
        Scope::of(first),
        vec![a_span(1, "checkout"), a_span(2, "checkout")],
    )
    .await
    .expect("the first tenant's batch is accepted");
    sink.accept(Scope::of(second), vec![a_span(3, "payments")])
        .await
        .expect("the second tenant's batch is accepted");
}

async fn accepts_an_empty_batch<S: SpanSink>(sink: &S) {
    sink.accept(Scope::of(TenantId::new(Uuid::from_u128(9))), Vec::new())
        .await
        .expect("an empty batch is accepted");
}

#[tokio::test]
async fn the_memory_sink_honours_the_contract() {
    let sink = MemorySink::default();

    accepts_batches_from_several_tenants(&sink).await;
    accepts_an_empty_batch(&sink).await;

    assert_eq!(sink.accepted(), 3);
}

#[tokio::test]
async fn the_clickhouse_sink_honours_the_contract() {
    let Some(client) = prepared("ferristrace_sink_contract").await else {
        return;
    };
    let first = TenantId::new(Uuid::from_u128(1));
    let second = TenantId::new(Uuid::from_u128(2));
    let sink = ClickhouseSink::new(
        &client,
        BatchLimits {
            max_rows: 1,
            period: Duration::from_secs(60),
        },
        Duration::from_secs(60 * 60),
        FixedClock::at(NOW),
    );

    sink.accept(
        Scope::of(first),
        vec![a_span(1, "checkout"), a_span(2, "checkout")],
    )
    .await
    .expect("the first batch is accepted");
    assert_eq!(
        (sink.pending_rows().await, stored_rows(&client).await),
        (0, 2),
        "two rows written under a one row threshold must have been sent"
    );

    sink.accept(Scope::of(second), vec![a_span(3, "payments")])
        .await
        .expect("the second batch is accepted");
    assert_eq!(
        (sink.pending_rows().await, stored_rows(&client).await),
        (0, 3),
        "the row of the second tenant must have been sent too"
    );

    sink.accept(Scope::of(second), Vec::new())
        .await
        .expect("an empty batch is accepted");
    sink.flush().await.expect("the sink flushes");

    assert_eq!(
        stored_rows(&client).await,
        3,
        "an empty batch must add nothing"
    );
}

pub fn connected(url: &str) -> Client {
    let mut client = Client::default().with_url(url);
    if let Ok(user) = std::env::var("CLICKHOUSE_USER") {
        client = client.with_user(user);
    }
    if let Ok(password) = std::env::var("CLICKHOUSE_PASSWORD") {
        client = client.with_password(password);
    }
    client
}

pub async fn prepared(name: &str) -> Option<Client> {
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

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct Counted {
    value: u64,
}

pub async fn stored_rows(client: &Client) -> u64 {
    client
        .query("SELECT count() AS value FROM spans")
        .fetch_one::<Counted>()
        .await
        .expect("counting the stored spans")
        .value
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct Expiry {
    value: u32,
}

async fn stored_expire_at(client: &Client) -> u32 {
    client
        .query("SELECT min(expire_at) AS value FROM spans")
        .fetch_one::<Expiry>()
        .await
        .expect("reading the expiry")
        .value
}

fn sink_with(client: &Client, limits: BatchLimits) -> ClickhouseSink<FixedClock> {
    ClickhouseSink::new(
        client,
        limits,
        Duration::from_secs(7 * 24 * 60 * 60),
        FixedClock::at(NOW),
    )
}

#[tokio::test]
async fn the_row_threshold_flushes_without_an_explicit_call() {
    let Some(client) = prepared("ferristrace_sink_rows").await else {
        return;
    };
    let sink = sink_with(
        &client,
        BatchLimits {
            max_rows: 2,
            period: Duration::from_secs(60 * 60),
        },
    );

    sink.accept(
        Scope::of(TenantId::new(Uuid::from_u128(1))),
        vec![a_span(1, "checkout"), a_span(2, "checkout")],
    )
    .await
    .expect("the batch is accepted");

    assert_eq!(sink.pending_rows().await, 0, "the threshold did not flush");
    assert_eq!(stored_rows(&client).await, 2);
}

#[tokio::test]
async fn one_buffer_carries_rows_from_several_tenants() {
    let Some(client) = prepared("ferristrace_sink_shared").await else {
        return;
    };
    let sink = sink_with(
        &client,
        BatchLimits {
            max_rows: 1_000,
            period: Duration::from_secs(60 * 60),
        },
    );

    sink.accept(
        Scope::of(TenantId::new(Uuid::from_u128(1))),
        vec![a_span(1, "checkout")],
    )
    .await
    .expect("the first tenant is accepted");
    sink.accept(
        Scope::of(TenantId::new(Uuid::from_u128(2))),
        vec![a_span(2, "payments")],
    )
    .await
    .expect("the second tenant is accepted");

    assert_eq!(
        sink.pending_rows().await,
        2,
        "the two tenants must share one buffer"
    );
    assert_eq!(stored_rows(&client).await, 0, "nothing should be sent yet");

    sink.flush().await.expect("the sink flushes");

    assert_eq!(stored_rows(&client).await, 2);
}

#[tokio::test(start_paused = true)]
async fn the_interval_flushes_once_the_period_has_elapsed() {
    let Some(client) = prepared("ferristrace_sink_period").await else {
        return;
    };
    let sink = sink_with(
        &client,
        BatchLimits {
            max_rows: 1_000_000,
            period: Duration::from_secs(30),
        },
    );
    let tenant = TenantId::new(Uuid::from_u128(1));

    sink.accept(Scope::of(tenant), vec![a_span(1, "checkout")])
        .await
        .expect("the first batch is accepted");
    assert_eq!(sink.pending_rows().await, 1, "the row count did not flush");

    tokio::time::advance(Duration::from_secs(31)).await;
    sink.accept(Scope::of(tenant), vec![a_span(2, "checkout")])
        .await
        .expect("the second batch is accepted");

    assert_eq!(sink.pending_rows().await, 0, "the period did not flush");
    assert_eq!(stored_rows(&client).await, 2);
}

#[tokio::test]
async fn expire_at_is_the_clock_plus_the_retention() {
    let Some(client) = prepared("ferristrace_sink_expiry").await else {
        return;
    };
    let sink = sink_with(
        &client,
        BatchLimits {
            max_rows: 1,
            period: Duration::from_secs(60 * 60),
        },
    );

    sink.accept(
        Scope::of(TenantId::new(Uuid::from_u128(1))),
        vec![a_span(1, "checkout")],
    )
    .await
    .expect("the batch is accepted");

    let expected = u32::try_from(NOW + 7 * 24 * 3600).expect("the expiry fits");
    assert_eq!(stored_expire_at(&client).await, expected);
}

async fn expire_at_of(client: &Client, service: &str) -> u32 {
    client
        .query("SELECT min(expire_at) AS value FROM spans WHERE service_name = ?")
        .bind(service)
        .fetch_one::<Expiry>()
        .await
        .expect("reading the expiry of a service")
        .value
}

#[tokio::test]
async fn scopes_with_different_retention_write_different_expire_at() {
    let Some(client) = prepared("ferristrace_sink_scope_retention").await else {
        return;
    };
    let sink = sink_with(
        &client,
        BatchLimits {
            max_rows: 1,
            period: Duration::from_secs(60 * 60),
        },
    );
    let week = RetentionDays::try_from(7).expect("a week");
    let quarter = RetentionDays::try_from(90).expect("a quarter");
    let tenant = |seed| TenantId::new(Uuid::from_u128(seed));

    sink.accept(
        Scope::of(tenant(1)).retained_for(Some(week)),
        vec![a_span(1, "weekly")],
    )
    .await
    .expect("the weekly batch is accepted");
    sink.accept(
        Scope::of(tenant(2)).retained_for(Some(quarter)),
        vec![a_span(2, "quarterly")],
    )
    .await
    .expect("the quarterly batch is accepted");
    sink.accept(Scope::of(tenant(3)), vec![a_span(3, "default")])
        .await
        .expect("the default batch is accepted");

    let after = |days: u64| u32::try_from(NOW + days * 24 * 3600).expect("the expiry fits");
    assert_eq!(expire_at_of(&client, "weekly").await, after(7));
    assert_eq!(expire_at_of(&client, "quarterly").await, after(90));
    assert_eq!(expire_at_of(&client, "default").await, after(7));
}

#[tokio::test]
async fn a_timestamp_the_store_cannot_hold_is_refused() {
    let Some(client) = prepared("ferristrace_sink_timestamp").await else {
        return;
    };
    let sink = sink_with(&client, BatchLimits::default());
    let mut span = a_span(1, "checkout");
    span.start_unix_nanos = u64::MAX;

    let outcome = sink
        .accept(Scope::of(TenantId::new(Uuid::from_u128(1))), vec![span])
        .await;

    assert_eq!(
        outcome,
        Err(ferristrace_domain::SinkError::TimestampOutOfRange)
    );
}

async fn an_empty_database(name: &str) -> Option<String> {
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
    Some(url)
}

fn settings_for(url: &str, database: &str) -> SpanStoreSettings {
    SpanStoreSettings {
        database: Some(database.to_owned()),
        user: std::env::var("CLICKHOUSE_USER").ok(),
        password: std::env::var("CLICKHOUSE_PASSWORD").ok(),
        ..SpanStoreSettings::at(url)
    }
}

#[tokio::test]
async fn opening_a_span_store_migrates_a_database_that_carries_no_schema() {
    let database = "span_store_opened";
    let Some(url) = an_empty_database(database).await else {
        return;
    };

    let store = open_span_store(&settings_for(&url, database), FixedClock::at(NOW))
        .await
        .expect("the span store opens");
    let sink = store.sink;
    sink.accept(
        Scope::of(TenantId::new(Uuid::from_u128(1))),
        vec![a_span(1, "checkout")],
    )
    .await
    .expect("a batch is accepted");
    sink.flush().await.expect("the batch commits");

    assert_eq!(
        stored_rows(&connected(&url).with_database(database)).await,
        1,
        "a store opened on an empty database must be ready to write"
    );
}

#[tokio::test]
async fn opening_a_span_store_twice_leaves_what_the_first_one_wrote() {
    let database = "span_store_reopened";
    let Some(url) = an_empty_database(database).await else {
        return;
    };
    let settings = settings_for(&url, database);

    let first = open_span_store(&settings, FixedClock::at(NOW))
        .await
        .expect("the first open")
        .sink;
    first
        .accept(
            Scope::of(TenantId::new(Uuid::from_u128(1))),
            vec![a_span(1, "checkout")],
        )
        .await
        .expect("a batch is accepted");
    first.flush().await.expect("the batch commits");

    let second = open_span_store(&settings, FixedClock::at(NOW))
        .await
        .expect("a second open must not refuse an applied schema")
        .sink;
    second
        .accept(
            Scope::of(TenantId::new(Uuid::from_u128(1))),
            vec![a_span(2, "checkout")],
        )
        .await
        .expect("a batch is accepted");
    second.flush().await.expect("the batch commits");

    assert_eq!(
        stored_rows(&connected(&url).with_database(database)).await,
        2,
        "reopening must migrate nothing and drop nothing"
    );
}

fn a_child(seed: u8, service: &str, root: &Span, start: u64, end: u64) -> Span {
    Span {
        trace_id: root.trace_id,
        parent: Some(root.span_id),
        service_name: service.into(),
        start_unix_nanos: start,
        end_unix_nanos: end,
        ..a_span(seed, service)
    }
}

async fn a_written_store(database: &str) -> Option<(String, ClickhouseReader)> {
    let url = an_empty_database(database).await?;
    let store = open_span_store(&settings_for(&url, database), FixedClock::at(NOW))
        .await
        .expect("the span store opens");
    Some((url, store.reader))
}

#[tokio::test]
async fn a_written_trace_is_read_back_whole_and_in_order() {
    let Some((url, reader)) = a_written_store("read_one_trace").await else {
        return;
    };
    let store = open_span_store(&settings_for(&url, "read_one_trace"), FixedClock::at(NOW))
        .await
        .expect("a sink")
        .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let root = a_span(1, "checkout");
    let child = a_child(
        2,
        "payments",
        &root,
        root.start_unix_nanos + 1_000,
        root.end_unix_nanos,
    );
    let trace = root.trace_id;
    store
        .accept(Scope::of(tenant), vec![child.clone(), root.clone()])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let found = reader
        .trace(tenant, trace, None)
        .await
        .expect("the trace reads");

    assert_eq!(found.len(), 2);
    assert_eq!(found[0].span_id, root.span_id, "the root comes first");
    assert_eq!(found[0].parent, None);
    assert_eq!(found[1].parent, Some(root.span_id));
    assert_eq!(&*found[1].service_name, "payments");
    assert_eq!(found[0].kind, root.kind);
    assert_eq!(found[0].status, root.status);
    assert_eq!(
        found[0].attributes.get("http.status_code").map(|v| &**v),
        Some("200"),
        "attributes survive the round trip"
    );
}

#[tokio::test]
async fn a_trace_belonging_to_another_tenant_is_not_read() {
    let Some((url, reader)) = a_written_store("read_other_tenant").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_other_tenant"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let mine = TenantId::new(Uuid::from_u128(1));
    let theirs = TenantId::new(Uuid::from_u128(2));
    let span = a_span(1, "checkout");
    store
        .accept(Scope::of(mine), vec![span.clone()])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    assert_eq!(
        reader
            .trace(mine, span.trace_id, None)
            .await
            .expect("a read")
            .len(),
        1
    );
    assert!(
        reader
            .trace(theirs, span.trace_id, None)
            .await
            .expect("a read")
            .is_empty(),
        "a trace id is not a capability: another tenant must see nothing"
    );
}

#[tokio::test]
async fn a_trace_nobody_wrote_reads_as_empty() {
    let Some((_, reader)) = a_written_store("read_absent_trace").await else {
        return;
    };

    let found = reader
        .trace(
            TenantId::new(Uuid::from_u128(1)),
            TraceId::try_from([9u8; 16].as_slice()).expect("sixteen bytes"),
            None,
        )
        .await
        .expect("a read");

    assert!(found.is_empty());
}

#[tokio::test]
async fn the_trace_list_summarises_each_trace_once() {
    let Some((url, reader)) = a_written_store("read_trace_list").await else {
        return;
    };
    let store = open_span_store(&settings_for(&url, "read_trace_list"), FixedClock::at(NOW))
        .await
        .expect("a sink")
        .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let root = a_span(1, "checkout");
    let child = a_child(
        2,
        "payments",
        &root,
        root.start_unix_nanos + 1_000,
        root.end_unix_nanos,
    );
    store
        .accept(Scope::of(tenant), vec![root.clone(), child])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let found = reader
        .traces(tenant, &a_wide_filter())
        .await
        .expect("the list reads");

    assert_eq!(found.len(), 1, "two spans of one trace are one row");
    let summary = &found[0];
    assert_eq!(summary.trace_id, root.trace_id);
    assert_eq!(
        &*summary.root_service, "checkout",
        "the parentless span names the trace"
    );
    assert_eq!(summary.spans, 2);
    assert_eq!(summary.errors, 0);
    assert_eq!(summary.started_unix_nanos, root.start_unix_nanos);
    assert_eq!(
        summary.duration_nanos,
        root.end_unix_nanos - root.start_unix_nanos
    );
}

#[tokio::test]
async fn the_trace_list_counts_errors_and_can_keep_only_traces_that_have_them() {
    let Some((url, reader)) = a_written_store("read_trace_errors").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_trace_errors"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let healthy = a_span(1, "checkout");
    let mut broken = a_span(3, "checkout");
    broken.status = SpanStatus::Error;
    store
        .accept(Scope::of(tenant), vec![healthy.clone(), broken.clone()])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let all = reader
        .traces(tenant, &a_wide_filter())
        .await
        .expect("the list reads");
    let mut only_errors = a_wide_filter();
    only_errors.only_errors = true;
    let failing = reader
        .traces(tenant, &only_errors)
        .await
        .expect("the list reads");

    assert_eq!(
        all.len(),
        2,
        "two spans with different trace ids are two traces"
    );
    assert_eq!(failing.len(), 1);
    assert_eq!(failing[0].trace_id, broken.trace_id);
    assert_eq!(failing[0].errors, 1);
}

#[tokio::test]
async fn the_trace_list_can_be_narrowed_to_a_service_that_appears_anywhere_in_it() {
    let Some((url, reader)) = a_written_store("read_trace_service").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_trace_service"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let root = a_span(1, "checkout");
    let child = a_child(
        2,
        "payments",
        &root,
        root.start_unix_nanos + 1_000,
        root.end_unix_nanos,
    );
    store
        .accept(Scope::of(tenant), vec![root.clone(), child])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let mut deep = a_wide_filter();
    deep.service = Some("payments".into());
    let mut absent = a_wide_filter();
    absent.service = Some("nobody".into());

    let found = reader.traces(tenant, &deep).await.expect("a read");

    assert_eq!(
        found.len(),
        1,
        "a service that is not the root still finds its trace"
    );
    assert_eq!(&*found[0].root_service, "checkout");
    assert_eq!(
        found[0].spans, 2,
        "the whole trace is summarised, not the match"
    );
    assert!(
        reader
            .traces(tenant, &absent)
            .await
            .expect("a read")
            .is_empty()
    );
}

#[tokio::test]
async fn a_window_that_misses_the_spans_finds_nothing() {
    let Some((url, reader)) = a_written_store("read_trace_window").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_trace_window"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let span = a_span(1, "checkout");
    store
        .accept(Scope::of(tenant), vec![span.clone()])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let before = TraceFilter::over(Window::new(1, span.start_unix_nanos).expect("a window"));
    let after = TraceFilter::over(
        Window::new(span.end_unix_nanos + 1, span.end_unix_nanos + 1_000).expect("a window"),
    );

    assert!(
        reader
            .traces(tenant, &before)
            .await
            .expect("a read")
            .is_empty()
    );
    assert!(
        reader
            .traces(tenant, &after)
            .await
            .expect("a read")
            .is_empty()
    );
}

#[tokio::test]
async fn the_trace_list_never_crosses_tenants() {
    let Some((url, reader)) = a_written_store("read_trace_tenants").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_trace_tenants"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let mine = TenantId::new(Uuid::from_u128(1));
    let theirs = TenantId::new(Uuid::from_u128(2));
    store
        .accept(Scope::of(mine), vec![a_span(1, "checkout")])
        .await
        .expect("a batch is accepted");
    store
        .accept(Scope::of(theirs), vec![a_span(2, "billing")])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let ours = reader.traces(mine, &a_wide_filter()).await.expect("a read");

    assert_eq!(ours.len(), 1);
    assert_eq!(&*ours[0].root_service, "checkout");
}

#[tokio::test]
async fn the_trace_list_can_be_narrowed_to_a_span_name_anywhere_in_it() {
    let Some((url, reader)) = a_written_store("read_trace_names").await else {
        return;
    };
    let store = open_span_store(&settings_for(&url, "read_trace_names"), FixedClock::at(NOW))
        .await
        .expect("a sink")
        .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let charging = a_span(1, "checkout");
    let deep = Span {
        name: "psp.charge".into(),
        ..a_child(
            2,
            "payments",
            &charging,
            charging.start_unix_nanos + 1,
            charging.end_unix_nanos,
        )
    };
    let unrelated = a_span(5, "checkout");
    store
        .accept(
            Scope::of(tenant),
            vec![charging.clone(), deep, unrelated.clone()],
        )
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let mut named = a_wide_filter();
    named.name = Some("psp.charge".into());
    let found = reader.traces(tenant, &named).await.expect("the list reads");

    assert_eq!(found.len(), 1, "only the trace that contains that span");
    assert_eq!(found[0].trace_id, charging.trace_id);
    assert_eq!(
        &*found[0].root_name, "work",
        "the trace is still named by its root, not by what was searched for"
    );
}

#[tokio::test]
async fn a_span_name_matches_on_part_of_it_and_ignores_case() {
    let Some((url, reader)) = a_written_store("read_trace_part_name").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_trace_part_name"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let span = Span {
        name: "PSP.Charge".into(),
        ..a_span(1, "checkout")
    };
    store
        .accept(Scope::of(tenant), vec![span.clone()])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    for typed in ["psp", "CHARGE", "p.ch"] {
        let mut named = a_wide_filter();
        named.name = Some(typed.into());
        let found = reader.traces(tenant, &named).await.expect("the list reads");
        assert_eq!(found.len(), 1, "{typed} should have found it");
    }

    let mut missing = a_wide_filter();
    missing.name = Some("stripe".into());
    assert!(
        reader
            .traces(tenant, &missing)
            .await
            .expect("the list reads")
            .is_empty(),
        "a name nothing carries finds nothing"
    );
}

#[tokio::test]
async fn a_name_and_a_service_both_have_to_match() {
    let Some((url, reader)) = a_written_store("read_trace_name_and_service").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_trace_name_and_service"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let root = a_span(1, "checkout");
    let charge = Span {
        name: "psp.charge".into(),
        ..a_child(
            2,
            "payments",
            &root,
            root.start_unix_nanos + 1,
            root.end_unix_nanos,
        )
    };
    store
        .accept(Scope::of(tenant), vec![root.clone(), charge])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let both = |service: &str, name: &str| {
        let mut filter = a_wide_filter();
        filter.service = Some(service.into());
        filter.name = Some(name.into());
        filter
    };

    assert_eq!(
        reader
            .traces(tenant, &both("payments", "psp.charge"))
            .await
            .expect("the list reads")
            .len(),
        1,
        "the trace has both"
    );
    assert!(
        reader
            .traces(tenant, &both("payments", "nothing.here"))
            .await
            .expect("the list reads")
            .is_empty(),
        "the service matches and the name does not, so neither does the trace"
    );
}

#[tokio::test]
async fn the_trace_list_carries_the_http_code_of_the_root_span() {
    let Some((url, reader)) = a_written_store("read_trace_code").await else {
        return;
    };
    let store = open_span_store(&settings_for(&url, "read_trace_code"), FixedClock::at(NOW))
        .await
        .expect("a sink")
        .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let mut root = a_span(1, "checkout");
    root.attributes
        .insert("http.status_code".into(), "503".into());
    let child = a_child(
        2,
        "payments",
        &root,
        root.start_unix_nanos + 1_000,
        root.end_unix_nanos,
    );
    let mut queued = a_span(3, "mailer");
    queued.attributes.remove("http.status_code");
    store
        .accept(Scope::of(tenant), vec![root.clone(), child, queued.clone()])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let found = reader
        .traces(tenant, &a_wide_filter())
        .await
        .expect("a read");
    let coded = found
        .iter()
        .find(|summary| summary.trace_id == root.trace_id)
        .expect("the trace with a code");
    let uncoded = found
        .iter()
        .find(|summary| summary.trace_id == queued.trace_id)
        .expect("the trace without a code");

    assert_eq!(
        coded.status_code,
        Some(503),
        "the root's code, not a child's"
    );
    assert_eq!(uncoded.status_code, None);
}

fn a_wide_filter() -> TraceFilter {
    TraceFilter::over(
        Window::new(1_600_000_000_000_000_000, 1_800_000_000_000_000_000).expect("a window"),
    )
}

#[tokio::test]
async fn the_services_table_measures_only_the_spans_a_service_was_called_on() {
    let Some((url, reader)) = a_written_store("read_services").await else {
        return;
    };
    let store = open_span_store(&settings_for(&url, "read_services"), FixedClock::at(NOW))
        .await
        .expect("a sink")
        .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let root = a_span(1, "checkout");
    let mut slow = a_span(3, "checkout");
    slow.end_unix_nanos = slow.start_unix_nanos + 4_000_000_000;
    slow.status = SpanStatus::Error;
    let mut outgoing = a_child(
        4,
        "checkout",
        &root,
        root.start_unix_nanos,
        root.end_unix_nanos,
    );
    outgoing.kind = SpanKind::Client;
    store
        .accept(Scope::of(tenant), vec![root.clone(), slow, outgoing])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let found = reader
        .services(tenant, &services_in(None))
        .await
        .expect("the services read");

    assert_eq!(found.len(), 1);
    let service = &found[0];
    assert_eq!(&*service.name, "checkout");
    assert_eq!(
        service.requests, 2,
        "the client span is work this service asked of another, not a request it served"
    );
    assert_eq!(service.errors, 1);
    assert!(service.p50_nanos > 0);
    assert!(
        service.p99_nanos >= service.p50_nanos,
        "p99 {} is below p50 {}",
        service.p99_nanos,
        service.p50_nanos
    );
    assert_eq!(service.last_seen_unix_nanos, root.start_unix_nanos);
}

#[tokio::test]
async fn the_services_table_never_crosses_tenants() {
    let Some((url, reader)) = a_written_store("read_services_tenants").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_services_tenants"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let mine = TenantId::new(Uuid::from_u128(1));
    store
        .accept(Scope::of(mine), vec![a_span(1, "checkout")])
        .await
        .expect("a batch is accepted");
    store
        .accept(
            Scope::of(TenantId::new(Uuid::from_u128(2))),
            vec![a_span(2, "billing")],
        )
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let found = reader
        .services(mine, &services_in(None))
        .await
        .expect("the services read");

    assert_eq!(found.len(), 1);
    assert_eq!(&*found[0].name, "checkout");
}

fn services_in(project: Option<ProjectId>) -> ServiceFilter {
    let mut filter = ServiceFilter::over(a_wide_window());
    filter.project = project;
    filter
}

fn a_wide_window() -> Window {
    Window::new(1_600_000_000_000_000_000, 1_800_000_000_000_000_000).expect("a window")
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct Owner {
    #[serde(with = "clickhouse::serde::uuid")]
    value: Uuid,
}

async fn stored_project(client: &Client) -> Uuid {
    client
        .query("SELECT any(project_id) AS value FROM spans")
        .fetch_one::<Owner>()
        .await
        .expect("reading the project")
        .value
}

#[tokio::test]
async fn a_span_carries_the_project_the_key_named() {
    let Some(client) = prepared("ferristrace_sink_project").await else {
        return;
    };
    let project = ProjectId::new(Uuid::from_u128(42));
    let scope = Scope::of(TenantId::new(Uuid::from_u128(1))).in_project(Some(project));
    let sink = sink_with(
        &client,
        BatchLimits {
            max_rows: 1,
            period: Duration::from_secs(60),
        },
    );

    sink.accept(scope, vec![a_span(1, "checkout")])
        .await
        .expect("the batch is accepted");

    assert_eq!(stored_project(&client).await, project.as_uuid());
}

#[tokio::test]
async fn a_span_from_a_key_naming_no_project_carries_none() {
    let Some(client) = prepared("ferristrace_sink_no_project").await else {
        return;
    };
    let sink = sink_with(
        &client,
        BatchLimits {
            max_rows: 1,
            period: Duration::from_secs(60),
        },
    );

    sink.accept(
        Scope::of(TenantId::new(Uuid::from_u128(1))),
        vec![a_span(1, "checkout")],
    )
    .await
    .expect("the batch is accepted");

    assert_eq!(
        stored_project(&client).await,
        Uuid::nil(),
        "no project is the only honest answer a project filter can be given"
    );
}

#[tokio::test]
async fn a_project_narrows_what_the_services_table_and_the_trace_list_answer() {
    let Some((url, reader)) = a_written_store("read_by_project").await else {
        return;
    };
    let store = open_span_store(&settings_for(&url, "read_by_project"), FixedClock::at(NOW))
        .await
        .expect("a sink")
        .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let storefront = ProjectId::new(Uuid::from_u128(11));
    let payments = ProjectId::new(Uuid::from_u128(12));
    store
        .accept(
            Scope::of(tenant).in_project(Some(storefront)),
            vec![a_span(1, "checkout")],
        )
        .await
        .expect("the first project");
    store
        .accept(
            Scope::of(tenant).in_project(Some(payments)),
            vec![a_span(2, "billing")],
        )
        .await
        .expect("the second project");
    store.flush().await.expect("the batch commits");

    let everything = reader
        .services(tenant, &services_in(None))
        .await
        .expect("a read");
    let only_storefront = reader
        .services(tenant, &services_in(Some(storefront)))
        .await
        .expect("a read");
    let mut narrowed = a_wide_filter();
    narrowed.project = Some(payments);

    assert_eq!(everything.len(), 2, "no project asked for means every one");
    assert_eq!(only_storefront.len(), 1);
    assert_eq!(&*only_storefront[0].name, "checkout");

    let traces = reader.traces(tenant, &narrowed).await.expect("a read");
    assert_eq!(traces.len(), 1);
    assert_eq!(&*traces[0].root_service, "billing");
}

#[tokio::test]
async fn a_project_nobody_wrote_under_answers_nothing_rather_than_everything() {
    let Some((url, reader)) = a_written_store("read_absent_project").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_absent_project"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    store
        .accept(
            Scope::of(tenant).in_project(Some(ProjectId::new(Uuid::from_u128(11)))),
            vec![a_span(1, "checkout")],
        )
        .await
        .expect("a batch");
    store.flush().await.expect("the batch commits");

    let stranger = Some(ProjectId::new(Uuid::from_u128(99)));
    let mut narrowed = a_wide_filter();
    narrowed.project = stranger;

    assert!(
        reader
            .services(tenant, &services_in(stranger))
            .await
            .expect("a read")
            .is_empty()
    );
    assert!(
        reader
            .traces(tenant, &narrowed)
            .await
            .expect("a read")
            .is_empty()
    );
}

fn with_events(span: Span, events: Vec<SpanEvent>) -> Span {
    Span { events, ..span }
}

fn an_exception(at: u64, kind: &str) -> SpanEvent {
    SpanEvent {
        time_unix_nanos: at,
        name: "exception".into(),
        attributes: BTreeMap::from([
            ("exception.type".into(), kind.into()),
            (
                "exception.message".into(),
                "deadline exceeded after 960ms".into(),
            ),
            (
                "exception.stacktrace".into(),
                "at charge (psp/client.ts:88)".into(),
            ),
        ]),
    }
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct CountedEvents {
    value: u64,
}

async fn events_expire_at(client: &Client) -> u32 {
    client
        .query("SELECT min(expire_at) AS value FROM span_events")
        .fetch_one::<Expiry>()
        .await
        .expect("reading the event expiry")
        .value
}

async fn stored_events(client: &Client) -> u64 {
    client
        .query("SELECT count() AS value FROM span_events")
        .fetch_one::<CountedEvents>()
        .await
        .expect("counting the stored events")
        .value
}

#[tokio::test]
async fn an_exception_written_with_a_span_reads_back_on_that_span() {
    let Some((url, reader)) = a_written_store("read_span_events").await else {
        return;
    };
    let store = open_span_store(&settings_for(&url, "read_span_events"), FixedClock::at(NOW))
        .await
        .expect("a sink")
        .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let root = a_span(1, "checkout");
    let failed = with_events(
        a_child(
            2,
            "payments",
            &root,
            root.start_unix_nanos + 1_000,
            root.end_unix_nanos,
        ),
        vec![an_exception(
            root.start_unix_nanos + 9_000,
            "PspTimeoutError",
        )],
    );
    let trace = root.trace_id;
    store
        .accept(Scope::of(tenant), vec![root.clone(), failed.clone()])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let found = reader
        .trace(tenant, trace, None)
        .await
        .expect("the trace reads");

    assert_eq!(found.len(), 2);
    assert!(found[0].events.is_empty(), "the root recorded nothing");
    assert_eq!(found[1].events.len(), 1);
    let event = &found[1].events[0];
    assert_eq!(&*event.name, "exception");
    assert_eq!(event.time_unix_nanos, root.start_unix_nanos + 9_000);
    assert_eq!(
        event.attributes.get("exception.type").map(|v| &**v),
        Some("PspTimeoutError")
    );
    assert_eq!(
        event.attributes.get("exception.stacktrace").map(|v| &**v),
        Some("at charge (psp/client.ts:88)")
    );
}

#[tokio::test]
async fn several_events_on_one_span_read_back_in_time_order() {
    let Some((url, reader)) = a_written_store("read_events_order").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_events_order"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let base = a_span(1, "checkout");
    let span = with_events(
        base.clone(),
        vec![
            an_exception(base.start_unix_nanos + 30_000, "Last"),
            SpanEvent {
                time_unix_nanos: base.start_unix_nanos + 10_000,
                name: "retrying".into(),
                attributes: BTreeMap::from([("attempt".into(), "2".into())]),
            },
        ],
    );
    store
        .accept(Scope::of(tenant), vec![span])
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let found = reader
        .trace(tenant, base.trace_id, None)
        .await
        .expect("the trace reads");

    assert_eq!(
        found[0]
            .events
            .iter()
            .map(|event| &*event.name)
            .collect::<Vec<_>>(),
        vec!["retrying", "exception"],
        "the store orders them, not the batch"
    );
}

#[tokio::test]
async fn another_organisations_events_are_not_read_with_a_shared_trace_id() {
    let Some((url, reader)) = a_written_store("read_events_tenant").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_events_tenant"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let mine = TenantId::new(Uuid::from_u128(1));
    let theirs = TenantId::new(Uuid::from_u128(2));
    let base = a_span(1, "checkout");
    store
        .accept(
            Scope::of(theirs),
            vec![with_events(
                base.clone(),
                vec![an_exception(base.start_unix_nanos + 1, "TheirSecret")],
            )],
        )
        .await
        .expect("a batch is accepted");
    store.flush().await.expect("the batch commits");

    let found = reader
        .trace(mine, base.trace_id, None)
        .await
        .expect("the trace reads");

    assert!(found.is_empty(), "no span, and so no event either");
}

#[tokio::test]
async fn spans_with_no_events_write_no_event_rows() {
    let Some(client) = prepared("ferristrace_no_events").await else {
        return;
    };
    let sink = sink_with(&client, BatchLimits::default());

    sink.accept(
        Scope::of(TenantId::new(Uuid::from_u128(1))),
        vec![a_span(1, "checkout"), a_span(2, "checkout")],
    )
    .await
    .expect("a batch is accepted");
    sink.flush().await.expect("the batch commits");

    assert_eq!(stored_rows(&client).await, 2);
    assert_eq!(stored_events(&client).await, 0);
}

#[tokio::test]
async fn an_event_expires_with_the_span_that_recorded_it() {
    let Some(client) = prepared("ferristrace_event_expiry").await else {
        return;
    };
    let sink = sink_with(&client, BatchLimits::default());
    let base = a_span(1, "checkout");

    sink.accept(
        Scope::of(TenantId::new(Uuid::from_u128(1))),
        vec![with_events(
            base.clone(),
            vec![an_exception(base.start_unix_nanos + 1, "PspTimeoutError")],
        )],
    )
    .await
    .expect("a batch is accepted");
    sink.flush().await.expect("the batch commits");

    assert_eq!(
        events_expire_at(&client).await,
        stored_expire_at(&client).await,
    );
}

async fn collapsed_rows(client: &Client) -> u64 {
    client
        .query("SELECT count() AS value FROM spans FINAL")
        .fetch_one::<Counted>()
        .await
        .expect("counting the collapsed spans")
        .value
}

#[tokio::test]
async fn a_resent_export_is_read_back_once() {
    let Some(client) = prepared("ferristrace_span_sink_resent").await else {
        return;
    };
    let sink = sink_with(&client, BatchLimits::default());
    let tenant = TenantId::new(Uuid::from_u128(11));
    let batch = vec![a_span(4, "checkout"), a_span(5, "checkout")];

    sink.accept(Scope::of(tenant), batch.clone())
        .await
        .expect("the first export is accepted");
    sink.flush().await.expect("the first export is committed");
    sink.accept(Scope::of(tenant), batch)
        .await
        .expect("the retry is accepted");
    sink.flush().await.expect("the retry is committed");

    assert_eq!(collapsed_rows(&client).await, 2);
}

#[tokio::test]
async fn a_trace_read_for_one_project_carries_none_of_another_projects_spans_or_events() {
    let Some((url, reader)) = a_written_store("read_trace_by_project").await else {
        return;
    };
    let store = open_span_store(
        &settings_for(&url, "read_trace_by_project"),
        FixedClock::at(NOW),
    )
    .await
    .expect("a sink")
    .sink;
    let tenant = TenantId::new(Uuid::from_u128(1));
    let storefront = ProjectId::new(Uuid::from_u128(11));
    let payments = ProjectId::new(Uuid::from_u128(12));
    let elsewhere = ProjectId::new(Uuid::from_u128(13));
    let root = a_span(1, "checkout");
    let theirs = with_events(
        a_child(
            2,
            "billing",
            &root,
            root.start_unix_nanos + 1_000,
            root.end_unix_nanos,
        ),
        vec![an_exception(root.start_unix_nanos + 2_000, "TheirSecret")],
    );
    store
        .accept(
            Scope::of(tenant).in_project(Some(storefront)),
            vec![root.clone()],
        )
        .await
        .expect("the first project");
    store
        .accept(Scope::of(tenant).in_project(Some(payments)), vec![theirs])
        .await
        .expect("the second project");
    store.flush().await.expect("the batch commits");

    let whole = reader
        .trace(tenant, root.trace_id, None)
        .await
        .expect("a read");
    let ours = reader
        .trace(tenant, root.trace_id, Some(storefront))
        .await
        .expect("a read");
    let nobodys = reader
        .trace(tenant, root.trace_id, Some(elsewhere))
        .await
        .expect("a read");

    assert_eq!(whole.len(), 2, "no project asked for means the whole trace");
    assert_eq!(ours.len(), 1);
    assert_eq!(ours[0].span_id, root.span_id);
    assert!(ours.iter().all(|span| span.events.is_empty()));
    assert!(
        nobodys.is_empty(),
        "a trace no span of the project holds reads like an unknown one"
    );
}
