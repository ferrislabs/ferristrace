use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseMigrator, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::{
    AttributeKey, Clause, Expr, Field, Number, Op, Query, Scalar, Scope, SpanReader, SpanSink,
    TenantId, TraceFilter, Value, Window,
};
use ferristrace_infra::FixedClock;
use ferristrace_otlp::spans_of;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value as Any};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as ProtoSpan, Status};
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const BASE: u64 = 1_700_000_000_000_000_000;
const SECOND: u64 = 1_000_000_000;
const MILLI: u64 = 1_000_000;
const HOUR: u64 = 3_600 * SECOND;
const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MINE: u128 = 1;
const THEIRS: u128 = 2;

fn tenant(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn the_hour() -> Window {
    Window::new(BASE, BASE + HOUR).expect("an hour")
}

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

fn text(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(Any::StringValue(value.to_owned())),
        }),
        ..KeyValue::default()
    }
}

fn int(key: &str, value: i64) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(Any::IntValue(value)),
        }),
        ..KeyValue::default()
    }
}

struct Seen {
    trace: u8,
    span: u8,
    service: &'static str,
    attributes: Vec<KeyValue>,
    failed: bool,
    millis: u64,
}

impl Seen {
    fn proto(&self) -> ProtoSpan {
        let start = BASE + u64::from(self.trace) * SECOND + u64::from(self.span) * MILLI;
        ProtoSpan {
            trace_id: vec![self.trace; 16],
            span_id: vec![self.span; 8],
            name: "work".to_owned(),
            kind: 2,
            start_time_unix_nano: start,
            end_time_unix_nano: start + self.millis * MILLI,
            attributes: self.attributes.clone(),
            status: self.failed.then(|| Status {
                code: 2,
                ..Status::default()
            }),
            ..ProtoSpan::default()
        }
    }
}

async fn sent(client: &Client, who: TenantId, seen: &[Seen]) {
    let mut by_service: Vec<(&str, Vec<ProtoSpan>)> = Vec::new();
    for one in seen {
        match by_service.iter_mut().find(|(name, _)| *name == one.service) {
            Some((_, spans)) => spans.push(one.proto()),
            None => by_service.push((one.service, vec![one.proto()])),
        }
    }
    let request = ExportTraceServiceRequest {
        resource_spans: by_service
            .into_iter()
            .map(|(service, spans)| ResourceSpans {
                resource: Some(Resource {
                    attributes: vec![text("service.name", service)],
                    ..Resource::default()
                }),
                scope_spans: vec![ScopeSpans {
                    spans,
                    ..ScopeSpans::default()
                }],
                ..ResourceSpans::default()
            })
            .collect(),
    };
    let sink = ClickhouseSink::new(
        client,
        BatchLimits {
            max_rows: 100_000,
            period: Duration::from_secs(60),
        },
        RETENTION,
        FixedClock::at(NOW),
    );
    sink.accept(Scope::of(who), spans_of(request).expect("a valid export"))
        .await
        .expect("spans accepted");
    sink.flush().await.expect("spans flushed");
}

fn corpus() -> Vec<Seen> {
    vec![
        Seen {
            trace: 1,
            span: 1,
            service: "checkout",
            attributes: vec![int("http.status_code", 200), text("rpc.method", "Pay")],
            failed: false,
            millis: 10,
        },
        Seen {
            trace: 2,
            span: 1,
            service: "checkout",
            attributes: vec![int("http.status_code", 503), text("rpc.method", "Refund")],
            failed: true,
            millis: 900,
        },
        Seen {
            trace: 3,
            span: 1,
            service: "payments",
            attributes: vec![text("rpc.method", "Pay"), int("retries", 4)],
            failed: false,
            millis: 30,
        },
        Seen {
            trace: 3,
            span: 2,
            service: "payments",
            attributes: vec![text("db.system", "postgres")],
            failed: false,
            millis: 5,
        },
        Seen {
            trace: 4,
            span: 1,
            service: "checkout",
            attributes: vec![
                text("rpc.method", "Pay"),
                text("note", "it's a \"quote\"; DROP TABLE spans"),
            ],
            failed: false,
            millis: 12,
        },
    ]
}

fn key(name: &str) -> Field {
    Field::Attribute(AttributeKey::new(name).expect("a key"))
}

fn word(value: &str) -> Scalar {
    Scalar::Text(value.into())
}

fn num(value: f64) -> Scalar {
    Scalar::Number(Number::new(value).expect("a finite number"))
}

fn is(field: Field, op: Op, value: Value) -> Expr {
    Expr::Is(Clause::new(field, op, value).expect("a valid clause"))
}

fn asking(expr: Expr) -> TraceFilter {
    let mut filter = TraceFilter::over(the_hour());
    filter.query = Query::new(expr).expect("a valid query");
    filter
}

async fn found(reader: &ClickhouseReader, who: TenantId, expr: Expr) -> Vec<u8> {
    let mut seeds: Vec<u8> = reader
        .traces(who, &asking(expr))
        .await
        .expect("the traces read")
        .into_iter()
        .map(|trace| trace.trace_id.as_bytes()[0])
        .collect();
    seeds.sort_unstable();
    seeds
}

async fn reader(name: &str) -> Option<ClickhouseReader> {
    let client = prepared(name).await?;
    sent(&client, tenant(MINE), &corpus()).await;
    sent(
        &client,
        tenant(THEIRS),
        &[Seen {
            trace: 9,
            span: 1,
            service: "checkout",
            attributes: vec![int("http.status_code", 503), text("rpc.method", "Pay")],
            failed: true,
            millis: 1,
        }],
    )
    .await;
    Some(ClickhouseReader::new(client))
}

#[tokio::test]
async fn a_number_comparison_on_a_built_in_keeps_only_the_503() {
    let Some(reader) = reader("ferristrace_query_http").await else {
        return;
    };

    let seeds = found(
        &reader,
        tenant(MINE),
        is(Field::HttpStatusCode, Op::Gte, Value::One(num(500.0))),
    )
    .await;

    assert_eq!(seeds, vec![2]);
}

#[tokio::test]
async fn one_span_has_to_satisfy_the_whole_expression() {
    let Some(reader) = reader("ferristrace_query_span").await else {
        return;
    };
    let pay_or_refund = Expr::Any(vec![
        is(key("rpc.method"), Op::Eq, Value::One(word("Pay"))),
        is(key("rpc.method"), Op::Eq, Value::One(word("Refund"))),
    ]);
    let in_checkout = is(Field::Service, Op::Eq, Value::One(word("checkout")));
    let on_one_span = Expr::All(vec![in_checkout, pay_or_refund]);

    let seeds = found(&reader, tenant(MINE), on_one_span).await;
    let split = found(
        &reader,
        tenant(MINE),
        Expr::All(vec![
            is(Field::Service, Op::Eq, Value::One(word("payments"))),
            is(key("db.system"), Op::Eq, Value::One(word("postgres"))),
            is(key("rpc.method"), Op::Eq, Value::One(word("Pay"))),
        ]),
    )
    .await;

    assert_eq!(seeds, vec![1, 2, 4]);
    assert_eq!(split, Vec::<u8>::new());
}

#[tokio::test]
async fn an_attribute_that_is_absent_is_found_with_not_exists() {
    let Some(reader) = reader("ferristrace_query_absent").await else {
        return;
    };

    let with = found(
        &reader,
        tenant(MINE),
        is(key("db.system"), Op::Exists, Value::Nothing),
    )
    .await;
    let without = found(
        &reader,
        tenant(MINE),
        Expr::Not(Box::new(is(key("rpc.method"), Op::Exists, Value::Nothing))),
    )
    .await;

    assert_eq!(with, vec![3]);
    assert_eq!(without, vec![3]);
}

#[tokio::test]
async fn a_hostile_key_or_value_is_data() {
    let Some(reader) = reader("ferristrace_query_hostile").await else {
        return;
    };
    let nasty = "it's a \"quote\"; DROP TABLE spans";

    let by_value = found(
        &reader,
        tenant(MINE),
        is(key("note"), Op::Eq, Value::One(word(nasty))),
    )
    .await;
    let by_key = found(
        &reader,
        tenant(MINE),
        is(key("a'] OR 1=1 --"), Op::Eq, Value::One(word("x"))),
    )
    .await;
    let still_there = found(
        &reader,
        tenant(MINE),
        is(key("rpc.method"), Op::Exists, Value::Nothing),
    )
    .await;

    assert_eq!(by_value, vec![4]);
    assert_eq!(by_key, Vec::<u8>::new());
    assert_eq!(still_there, vec![1, 2, 3, 4]);
}

#[tokio::test]
async fn another_organisation_never_matches() {
    let Some(reader) = reader("ferristrace_query_tenants").await else {
        return;
    };

    let mine = found(
        &reader,
        tenant(MINE),
        is(Field::Status, Op::Eq, Value::One(word("error"))),
    )
    .await;
    let theirs = found(
        &reader,
        tenant(THEIRS),
        is(Field::Status, Op::Eq, Value::One(word("error"))),
    )
    .await;

    assert_eq!(mine, vec![2]);
    assert_eq!(theirs, vec![9]);
}

#[tokio::test]
async fn a_text_attribute_compares_as_a_number_and_a_list_matches_any() {
    let Some(reader) = reader("ferristrace_query_numbers").await else {
        return;
    };

    let retried = found(
        &reader,
        tenant(MINE),
        is(key("retries"), Op::Gt, Value::One(num(3.0))),
    )
    .await;
    let listed = found(
        &reader,
        tenant(MINE),
        is(
            key("rpc.method"),
            Op::In,
            Value::Many(vec![word("Refund"), word("Pay")]),
        ),
    )
    .await;
    let slow = found(
        &reader,
        tenant(MINE),
        is(
            Field::Duration,
            Op::Gt,
            Value::One(num(500.0 * MILLI as f64)),
        ),
    )
    .await;
    let named = found(
        &reader,
        tenant(MINE),
        is(Field::Service, Op::Contains, Value::One(word("PAY"))),
    )
    .await;

    assert_eq!(retried, vec![3]);
    assert_eq!(listed, vec![1, 2, 3, 4]);
    assert_eq!(slow, vec![2]);
    assert_eq!(named, vec![3]);
}
