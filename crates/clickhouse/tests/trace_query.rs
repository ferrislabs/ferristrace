use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseMigrator, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::{
    AttributeFilter, AttributeKey, Clause, Expr, FacetFilter, Field, Number, Op, Query, Scalar,
    Scope, SpanFilter, SpanOrder, SpanReader, SpanSink, TenantId, TraceFilter, Value, Window,
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

#[tokio::test]
async fn the_catalogue_lists_the_keys_of_the_window_and_their_values() {
    let Some(reader) = reader("ferristrace_query_catalogue").await else {
        return;
    };
    let mut narrowed = AttributeFilter::over(the_hour());
    narrowed.prefix = Some("RPC".into());

    let keys = reader
        .attributes(tenant(MINE), &AttributeFilter::over(the_hour()))
        .await
        .expect("the keys read");
    let prefixed = reader
        .attributes(tenant(MINE), &narrowed)
        .await
        .expect("the keys read");
    let values = reader
        .attribute_values(
            tenant(MINE),
            &AttributeKey::new("rpc.method").expect("a key"),
            &AttributeFilter::over(the_hour()),
        )
        .await
        .expect("the values read");
    let theirs = reader
        .attributes(tenant(THEIRS), &AttributeFilter::over(the_hour()))
        .await
        .expect("the keys read");

    let named = |seen: &[ferristrace_domain::SeenAttribute]| -> Vec<(String, bool)> {
        let mut pairs: Vec<_> = seen
            .iter()
            .map(|k| (k.key.to_string(), k.numeric))
            .collect();
        pairs.sort();
        pairs
    };
    assert!(named(&keys).contains(&("http.status_code".to_owned(), true)));
    assert!(named(&keys).contains(&("rpc.method".to_owned(), false)));
    assert!(named(&keys).contains(&("db.system".to_owned(), false)));
    assert_eq!(named(&prefixed), vec![("rpc.method".to_owned(), false)]);
    assert_eq!(
        values
            .iter()
            .map(|v| (v.value.to_string(), v.spans))
            .collect::<Vec<_>>(),
        vec![("Pay".to_owned(), 3), ("Refund".to_owned(), 1)]
    );
    assert!(named(&theirs).iter().all(|(key, _)| key != "db.system"));
}

fn facet_filter(query: Query, fields: Vec<Field>, duration: bool) -> FacetFilter {
    FacetFilter::new(the_hour(), query, fields, duration).expect("a valid facet request")
}

#[tokio::test]
async fn a_facet_counts_the_values_of_a_field_under_the_query_without_its_own_clauses() {
    let Some(reader) = reader("ferristrace_query_facets").await else {
        return;
    };
    let method =
        Query::new(is(key("rpc.method"), Op::Eq, Value::One(word("Pay")))).expect("a query");
    let both = Query::new(Expr::All(vec![
        is(Field::Service, Op::Eq, Value::One(word("checkout"))),
        is(key("rpc.method"), Op::Eq, Value::One(word("Pay"))),
    ]))
    .expect("a query");

    let by_service = reader
        .facets(
            tenant(MINE),
            &facet_filter(method, vec![Field::Service], false),
        )
        .await
        .expect("the facets read");
    let by_method = reader
        .facets(
            tenant(MINE),
            &facet_filter(both, vec![key("rpc.method"), Field::Service], false),
        )
        .await
        .expect("the facets read");

    let counts = |values: &ferristrace_domain::FacetValues| -> Vec<(String, u64)> {
        values
            .values
            .iter()
            .map(|v| (v.value.to_string(), v.spans))
            .collect()
    };
    assert_eq!(
        counts(&by_service.fields[0]),
        vec![("checkout".to_owned(), 2), ("payments".to_owned(), 1)]
    );
    assert_eq!(
        counts(&by_method.fields[0]),
        vec![("Pay".to_owned(), 2), ("Refund".to_owned(), 1)],
        "the method facet ignores its own clause but keeps the service one"
    );
    assert_eq!(
        counts(&by_method.fields[1]),
        vec![("checkout".to_owned(), 2), ("payments".to_owned(), 1)],
        "the service facet ignores its own clause but keeps the method one"
    );
    assert!(by_service.duration.is_none());
}

#[tokio::test]
async fn a_facet_only_counts_spans_that_hold_the_attribute_and_never_crosses_tenants() {
    let Some(reader) = reader("ferristrace_query_facets_scope").await else {
        return;
    };

    let mine = reader
        .facets(
            tenant(MINE),
            &facet_filter(
                Query::anything(),
                vec![key("db.system"), Field::Status, Field::Kind],
                false,
            ),
        )
        .await
        .expect("the facets read");
    let theirs = reader
        .facets(
            tenant(THEIRS),
            &facet_filter(
                Query::anything(),
                vec![key("db.system"), Field::Status],
                false,
            ),
        )
        .await
        .expect("the facets read");

    assert_eq!(mine.fields[0].values.len(), 1);
    assert_eq!(&*mine.fields[0].values[0].value, "postgres");
    assert_eq!(
        mine.fields[1]
            .values
            .iter()
            .map(|v| (v.value.to_string(), v.spans))
            .collect::<Vec<_>>(),
        vec![("unset".to_owned(), 4), ("error".to_owned(), 1)]
    );
    assert_eq!(
        mine.fields[2].values.iter().map(|v| v.spans).sum::<u64>(),
        5
    );
    assert!(theirs.fields[0].values.is_empty());
    assert_eq!(theirs.fields[1].values.len(), 1);
}

#[tokio::test]
async fn the_duration_range_follows_the_query_but_not_its_own_clauses() {
    let Some(reader) = reader("ferristrace_query_facets_duration").await else {
        return;
    };
    let slow = Query::new(is(
        Field::Duration,
        Op::Gt,
        Value::One(num(500.0 * MILLI as f64)),
    ))
    .expect("a query");
    let payments = Query::new(Expr::All(vec![
        is(Field::Service, Op::Eq, Value::One(word("payments"))),
        is(
            Field::Duration,
            Op::Gt,
            Value::One(num(500.0 * MILLI as f64)),
        ),
    ]))
    .expect("a query");
    let nobody = Query::new(is(Field::Service, Op::Eq, Value::One(word("nope")))).expect("a query");

    let all = reader
        .facets(tenant(MINE), &facet_filter(slow, vec![], true))
        .await
        .expect("read");
    let narrowed = reader
        .facets(tenant(MINE), &facet_filter(payments, vec![], true))
        .await
        .expect("read");
    let none = reader
        .facets(tenant(MINE), &facet_filter(nobody, vec![], true))
        .await
        .expect("read");

    let range = all.duration.expect("a range");
    assert_eq!(range.min_nanos, 5 * MILLI);
    assert_eq!(range.max_nanos, 900 * MILLI);
    let payments_range = narrowed.duration.expect("a range");
    assert_eq!(
        (payments_range.min_nanos, payments_range.max_nanos),
        (5 * MILLI, 30 * MILLI)
    );
    assert!(none.duration.is_none());
}

#[test]
fn a_facet_request_is_bounded_and_refuses_ids_and_duration_as_facets() {
    let many = (0..13).map(|n| key(&format!("k{n}"))).collect();

    assert!(FacetFilter::new(the_hour(), Query::anything(), many, false).is_err());
    assert!(FacetFilter::new(the_hour(), Query::anything(), vec![Field::TraceId], false).is_err());
    assert!(FacetFilter::new(the_hour(), Query::anything(), vec![Field::Duration], false).is_err());
    assert!(FacetFilter::new(the_hour(), Query::anything(), vec![Field::Service], false).is_ok());
}

fn span_filter(query: Query, order: SpanOrder, limit: u32) -> SpanFilter {
    let mut filter = SpanFilter::over(the_hour()).at_most(limit);
    filter.query = query;
    filter.order = order;
    filter
}

#[tokio::test]
async fn the_spans_tab_lists_the_spans_that_match_not_the_traces_that_hold_them() {
    let Some(reader) = reader("ferristrace_query_spans").await else {
        return;
    };
    let method =
        Query::new(is(key("rpc.method"), Op::Eq, Value::One(word("Pay")))).expect("a query");

    let matched = reader
        .spans(tenant(MINE), &span_filter(method, SpanOrder::Started, 50))
        .await
        .expect("the spans read");
    let everything = reader
        .spans(
            tenant(MINE),
            &span_filter(Query::anything(), SpanOrder::Started, 50),
        )
        .await
        .expect("the spans read");

    let mut services: Vec<&str> = matched.iter().map(|span| &*span.service).collect();
    services.sort_unstable();
    assert_eq!(services, vec!["checkout", "checkout", "payments"]);
    assert_eq!(
        everything.len(),
        5,
        "the db span of trace 3 is a span of its own"
    );
    assert!(
        everything
            .iter()
            .any(|span| &*span.name == "work" && span.span_id.as_bytes()[0] == 2)
    );
}

#[tokio::test]
async fn spans_come_newest_or_slowest_first_within_the_limit_and_carry_their_fields() {
    let Some(reader) = reader("ferristrace_query_spans_order").await else {
        return;
    };

    let newest = reader
        .spans(
            tenant(MINE),
            &span_filter(Query::anything(), SpanOrder::Started, 50),
        )
        .await
        .expect("read");
    let slowest = reader
        .spans(
            tenant(MINE),
            &span_filter(Query::anything(), SpanOrder::Duration, 2),
        )
        .await
        .expect("read");

    assert!(
        newest
            .windows(2)
            .all(|pair| pair[0].started_unix_nanos >= pair[1].started_unix_nanos)
    );
    assert_eq!(slowest.len(), 2);
    assert_eq!(slowest[0].duration_nanos, 900 * MILLI);
    assert_eq!(slowest[0].status, ferristrace_domain::SpanStatus::Error);
    assert_eq!(slowest[0].status_code, Some(503));
    assert_eq!(slowest[0].kind, ferristrace_domain::SpanKind::Server);
    assert_eq!(&*slowest[0].service, "checkout");
    assert_eq!(slowest[0].trace_id.as_bytes()[0], 2);
    assert_eq!(slowest[1].duration_nanos, 30 * MILLI);
}

#[tokio::test]
async fn spans_never_cross_organisations_and_a_hostile_value_is_data() {
    let Some(reader) = reader("ferristrace_query_spans_scope").await else {
        return;
    };
    let nasty = Query::new(is(
        key("note"),
        Op::Eq,
        Value::One(word("it's a \"quote\"; DROP TABLE spans")),
    ))
    .expect("a query");
    let errors = Query::new(is(Field::Status, Op::Eq, Value::One(word("error")))).expect("a query");

    let hostile = reader
        .spans(tenant(MINE), &span_filter(nasty, SpanOrder::Started, 50))
        .await
        .expect("read");
    let mine = reader
        .spans(
            tenant(MINE),
            &span_filter(errors.clone(), SpanOrder::Started, 50),
        )
        .await
        .expect("read");
    let theirs = reader
        .spans(tenant(THEIRS), &span_filter(errors, SpanOrder::Started, 50))
        .await
        .expect("read");

    assert_eq!(hostile.len(), 1);
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].trace_id.as_bytes()[0], 2);
    assert_eq!(theirs.len(), 1);
    assert_eq!(theirs[0].trace_id.as_bytes()[0], 9);
}

#[test]
fn a_span_filter_is_bounded() {
    let filter = SpanFilter::over(the_hour());

    assert_eq!(filter.clone().at_most(0).limit(), 1);
    assert_eq!(filter.clone().at_most(10_000).limit(), 500);
    assert_eq!(filter.limit(), 50);
}
