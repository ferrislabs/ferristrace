use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{
    BatchLimits, ClickhouseLogReader, ClickhouseLogSink, ClickhouseMigrator, SpanStoreSettings,
    open_span_store,
};
use ferristrace_domain::{
    AttributeFilter, AttributeKey, Buckets, Clause, Expr, FacetFilter, Field, LogBucket, LogQuery,
    LogReader, LogRecord, LogSink, Op, ProjectId, Query, RetentionDays, Scalar, Scope, Severity,
    SpanId, TenantId, TraceId, Value, Window,
};
use ferristrace_infra::FixedClock;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const BASE: u64 = 1_700_000_000_000_000_000;
const SECOND: u64 = 1_000_000_000;
const DAY: u64 = 24 * 60 * 60;

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

fn sink_of(client: &Client) -> ClickhouseLogSink<FixedClock> {
    ClickhouseLogSink::new(
        client,
        BatchLimits {
            max_rows: 1,
            period: Duration::from_secs(60),
        },
        Duration::from_secs(30 * DAY),
        FixedClock::at(NOW),
    )
}

fn org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn trace(seed: u8) -> TraceId {
    TraceId::try_from([seed; 16].as_slice()).expect("sixteen bytes")
}

fn a_log(at_second: u64, service: &str, severity: Severity, body: &str) -> LogRecord {
    LogRecord {
        time_unix_nanos: BASE + at_second * SECOND,
        observed_unix_nanos: BASE + at_second * SECOND + 1,
        severity,
        severity_number: 0,
        service_name: service.into(),
        body: body.into(),
        attributes: BTreeMap::new(),
        resource: BTreeMap::from([("host.name".into(), "web-1".into())]),
        trace_id: None,
        span_id: None,
    }
}

fn the_hour() -> Window {
    Window::new(BASE, BASE + 3600 * SECOND).expect("an hour")
}

fn bodies(records: &[LogRecord]) -> Vec<&str> {
    records.iter().map(|record| &*record.body).collect()
}

async fn sent(sink: &ClickhouseLogSink<FixedClock>, scope: Scope, records: Vec<LogRecord>) {
    sink.accept_logs(scope, records)
        .await
        .expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
}

#[tokio::test]
async fn records_come_back_newest_first_with_everything_they_were_sent_with() {
    let Some(client) = prepared("ferristrace_logs_roundtrip").await else {
        return;
    };
    let sink = sink_of(&client);
    let mut traced = a_log(30, "checkout", Severity::Error, "payment declined");
    traced.severity_number = 17;
    traced.trace_id = Some(trace(7));
    traced.span_id = Some(SpanId::try_from([3u8; 8].as_slice()).expect("eight bytes"));
    traced.attributes = BTreeMap::from([("order.id".into(), "o-42".into())]);
    sent(
        &sink,
        Scope::of(org(1)),
        vec![
            a_log(10, "checkout", Severity::Info, "cart opened"),
            traced.clone(),
            a_log(20, "checkout", Severity::Warn, "slow psp"),
        ],
    )
    .await;

    let page = ClickhouseLogReader::new(client)
        .search(org(1), &LogQuery::over(the_hour()))
        .await
        .expect("the logs read back");

    assert_eq!(
        bodies(&page.records),
        vec!["payment declined", "slow psp", "cart opened"]
    );
    assert_eq!(page.records[0], traced);
    assert_eq!(page.records[2].trace_id, None);
    assert_eq!(page.records[2].span_id, None);
    assert_eq!(page.next, None, "everything fit on one page");
}

#[tokio::test]
async fn a_cursor_pages_through_records_that_share_a_time() {
    let Some(client) = prepared("ferristrace_logs_paging").await else {
        return;
    };
    let sink = sink_of(&client);
    let records = (0..7)
        .map(|index| {
            a_log(
                index / 2,
                "checkout",
                Severity::Info,
                &format!("line {index}"),
            )
        })
        .collect();
    sent(&sink, Scope::of(org(1)), records).await;
    let reader = ClickhouseLogReader::new(client);

    let mut seen = Vec::new();
    let mut query = LogQuery::over(the_hour()).at_most(3).expect("a limit");
    let mut pages = 0;
    loop {
        let page = reader.search(org(1), &query).await.expect("a page");
        pages += 1;
        assert!(page.records.len() <= 3);
        seen.extend(page.records.iter().map(|record| record.body.to_string()));
        let Some(next) = page.next else { break };
        query.before = Some(next);
        assert!(pages < 10, "the cursor must move");
    }

    seen.sort();
    assert_eq!(pages, 3);
    assert_eq!(
        seen,
        (0..7)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>(),
        "every record exactly once"
    );
}

async fn a_mixed_day(client: &Client) {
    let sink = sink_of(client);
    let mut keyed = a_log(40, "payments", Severity::Error, "Card DECLINED by issuer");
    keyed.attributes = BTreeMap::from([("http.route".into(), "/pay".into())]);
    keyed.trace_id = Some(trace(9));
    sent(
        &sink,
        Scope::of(org(1)).in_project(Some(project(30))),
        vec![
            a_log(10, "checkout", Severity::Info, "cart opened"),
            a_log(20, "checkout", Severity::Error, "cart lost"),
            a_log(30, "payments", Severity::Warn, "retrying the issuer"),
            keyed,
        ],
    )
    .await;
    sent(
        &sink,
        Scope::of(org(1)).in_project(Some(project(40))),
        vec![a_log(50, "checkout", Severity::Fatal, "out of memory")],
    )
    .await;
    sent(
        &sink,
        Scope::of(org(2)),
        vec![a_log(15, "checkout", Severity::Error, "somebody else's")],
    )
    .await;
}

#[tokio::test]
async fn every_filter_narrows_what_is_read() {
    let Some(client) = prepared("ferristrace_logs_filters").await else {
        return;
    };
    a_mixed_day(&client).await;
    let reader = ClickhouseLogReader::new(client);
    let found = async |query: LogQuery| {
        bodies(&reader.search(org(1), &query).await.expect("a page").records)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };

    let mut by_service = LogQuery::over(the_hour());
    by_service.services = vec!["payments".into()];
    let mut by_severity = LogQuery::over(the_hour());
    by_severity.severities = vec![Severity::Error, Severity::Fatal];
    let mut by_text = LogQuery::over(the_hour());
    by_text.text = Some("declined".into());
    let mut by_attribute = LogQuery::over(the_hour());
    by_attribute.attributes = vec![("http.route".into(), "/pay".into())];
    let mut by_trace = LogQuery::over(the_hour());
    by_trace.trace = Some(trace(9));
    let mut by_project = LogQuery::over(the_hour());
    by_project.project = Some(project(40));
    let narrow = Window::new(BASE + 15 * SECOND, BASE + 35 * SECOND).expect("a window");

    assert_eq!(
        found(by_service).await,
        vec!["Card DECLINED by issuer", "retrying the issuer"]
    );
    assert_eq!(
        found(by_severity).await,
        vec!["out of memory", "Card DECLINED by issuer", "cart lost"]
    );
    assert_eq!(found(by_text).await, vec!["Card DECLINED by issuer"]);
    assert_eq!(found(by_attribute).await, vec!["Card DECLINED by issuer"]);
    assert_eq!(found(by_trace).await, vec!["Card DECLINED by issuer"]);
    assert_eq!(found(by_project).await, vec!["out of memory"]);
    assert_eq!(
        found(LogQuery::over(narrow)).await,
        vec!["retrying the issuer", "cart lost"]
    );
}

#[tokio::test]
async fn the_logs_of_one_organisation_are_never_read_for_another() {
    let Some(client) = prepared("ferristrace_logs_isolation").await else {
        return;
    };
    a_mixed_day(&client).await;
    let reader = ClickhouseLogReader::new(client);

    let theirs = reader
        .search(org(2), &LogQuery::over(the_hour()))
        .await
        .expect("a page");
    let nobody = reader
        .search(org(3), &LogQuery::over(the_hour()))
        .await
        .expect("a page");
    let counted = reader
        .histogram(org(2), &LogQuery::over(the_hour()), Buckets::default())
        .await
        .expect("a histogram");

    assert_eq!(bodies(&theirs.records), vec!["somebody else's"]);
    assert!(nobody.records.is_empty());
    assert_eq!(counted.iter().map(LogBucket::total).sum::<u64>(), 1);
}

#[tokio::test]
async fn the_histogram_counts_each_severity_in_the_bucket_its_records_fall_in() {
    let Some(client) = prepared("ferristrace_logs_histogram").await else {
        return;
    };
    a_mixed_day(&client).await;
    let reader = ClickhouseLogReader::new(client);
    let minute = Window::new(BASE, BASE + 60 * SECOND).expect("a minute");
    let query = LogQuery::over(minute);

    let buckets = reader
        .histogram(org(1), &query, Buckets::try_from(6).expect("six"))
        .await
        .expect("a histogram");
    let page = reader.search(org(1), &query).await.expect("a page");

    assert_eq!(
        buckets
            .iter()
            .map(|b| b.start_unix_nanos)
            .collect::<Vec<_>>(),
        (0..6)
            .map(|slot| BASE + slot * 10 * SECOND)
            .collect::<Vec<_>>()
    );
    assert_eq!(buckets[0].total(), 0);
    assert_eq!(buckets[1].count(Severity::Info), 1);
    assert_eq!(buckets[2].count(Severity::Error), 1);
    assert_eq!(buckets[3].count(Severity::Warn), 1);
    assert_eq!(buckets[4].count(Severity::Error), 1);
    assert_eq!(buckets[5].count(Severity::Fatal), 1);
    for severity in Severity::ALL {
        let counted: u64 = buckets.iter().map(|b| b.count(severity)).sum();
        let listed = page
            .records
            .iter()
            .filter(|record| record.severity == severity)
            .count() as u64;
        assert_eq!(counted, listed, "{severity}");
    }

    let mut errors = query.clone();
    errors.severities = vec![Severity::Error];
    errors.project = Some(project(30));
    let narrowed = reader
        .histogram(org(1), &errors, Buckets::try_from(6).expect("six"))
        .await
        .expect("a histogram");
    assert_eq!(narrowed.iter().map(LogBucket::total).sum::<u64>(), 2);
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct Expiry {
    value: u32,
}

async fn expiry_of(client: &Client, service: &str) -> u32 {
    client
        .query("SELECT min(expire_at) AS value FROM logs WHERE service_name = ?")
        .bind(service)
        .fetch_one::<Expiry>()
        .await
        .expect("reading the expiry")
        .value
}

#[tokio::test]
async fn a_record_expires_after_the_retention_of_its_scope_or_the_default() {
    let Some(client) = prepared("ferristrace_logs_expiry").await else {
        return;
    };
    let sink = sink_of(&client);
    let week = RetentionDays::try_from(7).expect("a week");
    sent(
        &sink,
        Scope::of(org(1)).retained_for(Some(week)),
        vec![a_log(1, "kept-a-week", Severity::Info, "short")],
    )
    .await;
    sent(
        &sink,
        Scope::of(org(1)),
        vec![a_log(1, "kept-by-default", Severity::Info, "default")],
    )
    .await;

    assert_eq!(
        u64::from(expiry_of(&client, "kept-a-week").await),
        NOW + 7 * DAY
    );
    assert_eq!(
        u64::from(expiry_of(&client, "kept-by-default").await),
        NOW + 30 * DAY
    );
}

#[tokio::test]
async fn an_opened_span_store_writes_and_reads_logs_too() {
    let Some(url) = backend::clickhouse_url() else {
        return;
    };
    let database = "ferristrace_logs_opened";
    connected(&url)
        .query(&format!("DROP DATABASE IF EXISTS {database}"))
        .execute()
        .await
        .expect("dropping a leftover database");
    connected(&url)
        .query(&format!("CREATE DATABASE {database}"))
        .execute()
        .await
        .expect("creating the test database");
    let settings = SpanStoreSettings {
        database: Some(database.to_owned()),
        user: std::env::var("CLICKHOUSE_USER").ok(),
        password: std::env::var("CLICKHOUSE_PASSWORD").ok(),
        ..SpanStoreSettings::at(&url)
    };
    let store = open_span_store(&settings, FixedClock::at(NOW))
        .await
        .expect("the store opens");

    store
        .logs
        .accept_logs(
            Scope::of(org(1)),
            vec![a_log(1, "checkout", Severity::Info, "hello")],
        )
        .await
        .expect("the sink accepts");
    store.logs.flush().await.expect("the sink flushes");

    let page = store
        .log_reader
        .search(org(1), &LogQuery::over(the_hour()))
        .await
        .expect("a page");
    assert_eq!(bodies(&page.records), vec!["hello"]);
}

fn tree(expr: Expr) -> Query {
    Query::new(expr).expect("a valid query")
}

fn is(field: Field, op: Op, value: Value) -> Expr {
    Expr::Is(Clause::new(field, op, value).expect("a valid clause"))
}

fn text(word: &str) -> Value {
    Value::One(Scalar::Text(word.into()))
}

fn key(name: &str) -> Field {
    Field::Attribute(AttributeKey::new(name).expect("a valid key"))
}

async fn found(client: &Client, query: Query) -> Vec<String> {
    let mut asked = LogQuery::over(the_hour());
    asked.query = query;
    let page = ClickhouseLogReader::new(client.clone())
        .search(org(1), &asked)
        .await
        .expect("the logs read");
    page.records.iter().map(|r| r.body.to_string()).collect()
}

#[tokio::test]
async fn a_query_tree_filters_on_severity_body_and_attributes() {
    let Some(client) = prepared("ferristrace_logs_tree").await else {
        return;
    };
    a_mixed_day(&client).await;

    let errors = found(&client, tree(is(Field::Severity, Op::Eq, text("error")))).await;
    let either = found(
        &client,
        tree(Expr::Any(vec![
            is(Field::Severity, Op::Eq, text("fatal")),
            is(Field::Body, Op::Contains, text("declined")),
        ])),
    )
    .await;
    let routed = found(&client, tree(is(key("http.route"), Op::Eq, text("/pay")))).await;
    let resource = found(
        &client,
        tree(Expr::All(vec![
            is(key("host.name"), Op::Eq, text("web-1")),
            Expr::Not(Box::new(is(Field::Service, Op::Eq, text("checkout")))),
        ])),
    )
    .await;
    let absent = found(
        &client,
        tree(Expr::Not(Box::new(is(
            key("http.route"),
            Op::Exists,
            Value::Nothing,
        )))),
    )
    .await;

    assert_eq!(errors, ["Card DECLINED by issuer", "cart lost"]);
    assert_eq!(either, ["out of memory", "Card DECLINED by issuer"]);
    assert_eq!(routed, ["Card DECLINED by issuer"]);
    assert_eq!(resource, ["Card DECLINED by issuer", "retrying the issuer"]);
    assert!(!absent.contains(&"Card DECLINED by issuer".to_owned()));
    assert_eq!(absent.len(), 4);
}

#[tokio::test]
async fn a_key_with_a_quote_is_bound_as_data() {
    let Some(client) = prepared("ferristrace_logs_inject").await else {
        return;
    };
    a_mixed_day(&client).await;

    let none = found(&client, tree(is(key("x' OR 1=1 --"), Op::Eq, text("y")))).await;

    assert!(none.is_empty());
}

#[tokio::test]
async fn facets_count_values_without_the_clauses_on_their_own_field() {
    let Some(client) = prepared("ferristrace_logs_facets").await else {
        return;
    };
    a_mixed_day(&client).await;
    let filter = FacetFilter::new(
        the_hour(),
        tree(is(Field::Severity, Op::Eq, text("error"))),
        vec![Field::Service, Field::Severity],
        false,
    )
    .expect("a valid filter");

    let facets = ClickhouseLogReader::new(client)
        .facets(org(1), &filter)
        .await
        .expect("the facets read");

    let services = &facets.fields[0].values;
    let levels = &facets.fields[1].values;
    assert_eq!(services.len(), 2);
    assert!(services.iter().all(|seen| seen.spans == 1));
    assert!(levels.len() > 1, "severity ignores its own clause");
}

#[tokio::test]
async fn attributes_list_both_log_and_resource_keys_with_their_values() {
    let Some(client) = prepared("ferristrace_logs_attributes").await else {
        return;
    };
    a_mixed_day(&client).await;
    let reader = ClickhouseLogReader::new(client);
    let filter = AttributeFilter::over(the_hour());

    let keys = reader.attributes(org(1), &filter).await.expect("keys");
    let values = reader
        .attribute_values(
            org(1),
            &AttributeKey::new("host.name").expect("a key"),
            &filter,
        )
        .await
        .expect("values");

    let names: Vec<&str> = keys.iter().map(|seen| &*seen.key).collect();
    assert!(names.contains(&"host.name") && names.contains(&"http.route"));
    assert_eq!(values.len(), 1);
    assert_eq!(&*values[0].value, "web-1");
}
