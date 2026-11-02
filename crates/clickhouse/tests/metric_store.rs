use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{
    BatchLimits, ClickhouseMetricReader, ClickhouseMetricSink, ClickhouseMigrator,
    SpanStoreSettings, open_span_store,
};
use ferristrace_domain::{
    Aggregation, Exemplar, HistogramPoint, Metric, MetricData, MetricKind, MetricPoint,
    MetricQuery, MetricReader, MetricSink, ProjectId, RetentionDays, Scope, SeriesSet, SpanId,
    Temporality, TenantId, TraceId, Window,
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

fn sink_of(client: &Client) -> ClickhouseMetricSink<FixedClock> {
    ClickhouseMetricSink::new(
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

fn at(second: u64) -> u64 {
    BASE + second * SECOND
}

fn labelled(pairs: &[(&str, &str)]) -> BTreeMap<Box<str>, Box<str>> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect()
}

fn a_point(second: u64, value: f64, route: &str) -> MetricPoint {
    MetricPoint {
        time_unix_nanos: at(second),
        value,
        attributes: labelled(&[("http.route", route), ("host", "web-1")]),
        exemplars: Vec::new(),
    }
}

fn a_metric(name: &str, unit: &str, data: MetricData) -> Metric {
    Metric {
        name: name.into(),
        unit: unit.into(),
        description: format!("about {name}").into(),
        service_name: "checkout".into(),
        data,
    }
}

fn a_histogram(second: u64, bounds: &[f64], counts: Vec<u64>, sum: f64) -> HistogramPoint {
    HistogramPoint {
        time_unix_nanos: at(second),
        count: counts.iter().sum(),
        sum,
        min: None,
        max: None,
        bounds: bounds.to_vec(),
        counts,
        attributes: labelled(&[("http.route", "/pay")]),
        exemplars: Vec::new(),
    }
}

fn the_minute() -> Window {
    Window::new(at(0), at(60)).expect("a minute")
}

fn charting(name: &str, aggregation: Aggregation, group_by: &[&str]) -> MetricQuery {
    let mut query = MetricQuery::over(the_minute(), name);
    query.aggregation = Some(aggregation);
    query.group_by = group_by.iter().map(|key| (*key).into()).collect();
    query
}

async fn sent(sink: &ClickhouseMetricSink<FixedClock>, scope: Scope, metrics: Vec<Metric>) {
    sink.accept_metrics(scope, metrics)
        .await
        .expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
}

fn values_of(set: &SeriesSet, route: Option<&str>) -> Vec<(usize, f64)> {
    let series = set
        .series
        .iter()
        .find(|series| {
            route.is_none_or(|route| series.labels.get("http.route").map(|v| &**v) == Some(route))
        })
        .expect("the series");
    series
        .points
        .iter()
        .enumerate()
        .filter_map(|(slot, (_, value))| value.map(|value| (slot, value)))
        .collect()
}

#[tokio::test]
async fn the_catalogue_lists_each_kind_with_its_unit_in_name_order() {
    let Some(client) = prepared("ferristrace_metrics_catalogue").await else {
        return;
    };
    let sink = sink_of(&client);
    sent(
        &sink,
        Scope::of(org(1)),
        vec![
            a_metric(
                "queue.depth",
                "1",
                MetricData::Gauge(vec![a_point(1, 3.0, "/")]),
            ),
            a_metric(
                "http.requests",
                "{request}",
                MetricData::Sum {
                    monotonic: true,
                    temporality: Temporality::Delta,
                    points: vec![a_point(1, 1.0, "/")],
                },
            ),
            a_metric(
                "http.duration",
                "ms",
                MetricData::Histogram {
                    temporality: Temporality::Delta,
                    points: vec![a_histogram(1, &[10.0], vec![1, 0], 5.0)],
                },
            ),
        ],
    )
    .await;
    sent(
        &sink,
        Scope::of(org(1)),
        vec![a_metric(
            "queue.depth",
            "1",
            MetricData::Gauge(vec![a_point(9, 4.0, "/")]),
        )],
    )
    .await;
    sent(
        &sink,
        Scope::of(org(2)),
        vec![a_metric(
            "theirs",
            "1",
            MetricData::Gauge(vec![a_point(1, 1.0, "/")]),
        )],
    )
    .await;
    let reader = ClickhouseMetricReader::new(client);

    let listed = reader
        .catalogue(org(1), None, None)
        .await
        .expect("the catalogue");
    let searched = reader
        .catalogue(org(1), Some("HTTP"), None)
        .await
        .expect("the catalogue");

    assert_eq!(
        listed
            .iter()
            .map(|m| (&*m.name, m.kind, &*m.unit))
            .collect::<Vec<_>>(),
        vec![
            ("http.duration", MetricKind::Histogram, "ms"),
            ("http.requests", MetricKind::Sum, "{request}"),
            ("queue.depth", MetricKind::Gauge, "1"),
        ]
    );
    assert_eq!(&*listed[2].description, "about queue.depth");
    assert_eq!(listed[2].last_seen_unix_nanos, at(9));
    assert_eq!(
        searched.iter().map(|m| &*m.name).collect::<Vec<_>>(),
        vec!["http.duration", "http.requests"]
    );
}

#[tokio::test]
async fn a_gauge_grouped_by_an_attribute_returns_the_values_sent() {
    let Some(client) = prepared("ferristrace_metrics_gauge").await else {
        return;
    };
    sent(
        &sink_of(&client),
        Scope::of(org(1)),
        vec![a_metric(
            "queue.depth",
            "1",
            MetricData::Gauge(vec![
                a_point(5, 1.5, "/a"),
                a_point(5, 2.0, "/b"),
                a_point(20, 4.0, "/a"),
            ]),
        )],
    )
    .await;
    let reader = ClickhouseMetricReader::new(client);

    let grouped = reader
        .series(
            org(1),
            MetricKind::Gauge,
            Aggregation::Avg,
            &charting("queue.depth", Aggregation::Avg, &["http.route"]),
        )
        .await
        .expect("a series set");
    let whole = reader
        .series(
            org(1),
            MetricKind::Gauge,
            Aggregation::Sum,
            &charting("queue.depth", Aggregation::Sum, &[]),
        )
        .await
        .expect("a series set");

    assert_eq!(grouped.step_nanos, SECOND);
    assert_eq!(grouped.series.len(), 2);
    assert_eq!(grouped.series[0].labels, labelled(&[("http.route", "/a")]));
    assert_eq!(grouped.series[0].points.len(), 60);
    assert_eq!(grouped.series[0].points[5].0, at(5));
    assert_eq!(values_of(&grouped, Some("/a")), vec![(5, 1.5), (20, 4.0)]);
    assert_eq!(values_of(&grouped, Some("/b")), vec![(5, 2.0)]);
    assert_eq!(whole.series.len(), 1);
    assert!(whole.series[0].labels.is_empty());
    assert_eq!(values_of(&whole, None), vec![(5, 3.5), (20, 4.0)]);
}

#[tokio::test]
async fn a_delta_and_a_cumulative_counter_read_as_the_same_rate() {
    let Some(client) = prepared("ferristrace_metrics_rate").await else {
        return;
    };
    let delta = (1..=5)
        .map(|n| a_point(n * 10, 20.0, "/a"))
        .collect::<Vec<_>>();
    let cumulative = (0..=5)
        .map(|n| a_point(n * 10, 20.0 * n as f64, "/a"))
        .chain([a_point(55, 5.0, "/a")])
        .collect::<Vec<_>>();
    sent(
        &sink_of(&client),
        Scope::of(org(1)),
        vec![
            a_metric(
                "jobs.delta",
                "1",
                MetricData::Sum {
                    monotonic: true,
                    temporality: Temporality::Delta,
                    points: delta,
                },
            ),
            a_metric(
                "jobs.cumulative",
                "1",
                MetricData::Sum {
                    monotonic: true,
                    temporality: Temporality::Cumulative,
                    points: cumulative,
                },
            ),
        ],
    )
    .await;
    let reader = ClickhouseMetricReader::new(client);
    let rate_of = async |name: &str| {
        reader
            .series(
                org(1),
                MetricKind::Sum,
                Aggregation::Rate,
                &charting(name, Aggregation::Rate, &[]),
            )
            .await
            .expect("a series set")
    };

    let delta = rate_of("jobs.delta").await;
    let cumulative = rate_of("jobs.cumulative").await;

    assert_eq!(
        values_of(&delta, None),
        vec![(10, 20.0), (20, 20.0), (30, 20.0), (40, 20.0), (50, 20.0)],
        "a delta point is its increase over one step"
    );
    assert_eq!(
        values_of(&cumulative, None),
        vec![
            (10, 2.0),
            (20, 2.0),
            (30, 2.0),
            (40, 2.0),
            (50, 2.0),
            (55, 0.0)
        ],
        "a cumulative counter rises 20 every 10 s and its reset is clamped"
    );
}

#[tokio::test]
async fn a_cumulative_counter_sums_to_its_latest_value_per_step() {
    let Some(client) = prepared("ferristrace_metrics_sum").await else {
        return;
    };
    sent(
        &sink_of(&client),
        Scope::of(org(1)),
        vec![a_metric(
            "jobs",
            "1",
            MetricData::Sum {
                monotonic: true,
                temporality: Temporality::Cumulative,
                points: vec![
                    a_point(10, 5.0, "/a"),
                    a_point(10, 7.0, "/b"),
                    a_point(20, 9.0, "/a"),
                ],
            },
        )],
    )
    .await;

    let summed = ClickhouseMetricReader::new(client)
        .series(
            org(1),
            MetricKind::Sum,
            Aggregation::Sum,
            &charting("jobs", Aggregation::Sum, &[]),
        )
        .await
        .expect("a series set");

    assert_eq!(values_of(&summed, None), vec![(10, 12.0), (20, 9.0)]);
}

const BOUNDS: [f64; 9] = [
    100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0,
];
const WIDTH: f64 = 100.0;

fn bucketed(values: impl Iterator<Item = f64>) -> (Vec<u64>, f64) {
    let mut counts = vec![0u64; BOUNDS.len() + 1];
    let mut sum = 0.0;
    for value in values {
        let index = BOUNDS
            .iter()
            .position(|bound| value <= *bound)
            .unwrap_or(BOUNDS.len());
        counts[index] += 1;
        sum += value;
    }
    (counts, sum)
}

#[tokio::test]
async fn histogram_quantiles_land_within_one_bucket_of_the_true_quantile() {
    let Some(client) = prepared("ferristrace_metrics_quantiles").await else {
        return;
    };
    let skewed = || (1..=1000).map(|n| f64::from(n) * f64::from(n) / 1000.0);
    let (counts, sum) = bucketed(skewed());
    let mut point = a_histogram(10, &BOUNDS, counts, sum);
    point.min = Some(0.001);
    point.max = Some(1000.0);
    let mut sorted: Vec<f64> = skewed().collect();
    sorted.sort_by(f64::total_cmp);
    let truth = |per_mille: usize| sorted[per_mille - 1];
    sent(
        &sink_of(&client),
        Scope::of(org(1)),
        vec![a_metric(
            "http.duration",
            "ms",
            MetricData::Histogram {
                temporality: Temporality::Delta,
                points: vec![point],
            },
        )],
    )
    .await;
    let reader = ClickhouseMetricReader::new(client);
    let read = async |aggregation: Aggregation| {
        let set = reader
            .series(
                org(1),
                MetricKind::Histogram,
                aggregation,
                &charting("http.duration", aggregation, &["http.route"]),
            )
            .await
            .expect("a series set");
        assert_eq!(set.series[0].labels, labelled(&[("http.route", "/pay")]));
        values_of(&set, None)
    };

    let p50 = read(Aggregation::P50).await;
    let p95 = read(Aggregation::P95).await;
    let avg = read(Aggregation::Avg).await;
    let rate = read(Aggregation::Rate).await;

    assert_eq!(p50.len(), 1);
    assert_eq!(p50[0].0, 10);
    assert!(
        (p50[0].1 - truth(500)).abs() <= WIDTH,
        "{p50:?} vs {}",
        truth(500)
    );
    assert!(
        (p95[0].1 - truth(950)).abs() <= WIDTH,
        "{p95:?} vs {}",
        truth(950)
    );
    assert!((avg[0].1 - sum / 1000.0).abs() < 1e-9, "{avg:?}");
    assert_eq!(rate, vec![(10, 1000.0)]);
}

#[tokio::test]
async fn a_cumulative_histogram_is_read_from_the_difference_between_its_steps() {
    let Some(client) = prepared("ferristrace_metrics_cumulative_histogram").await else {
        return;
    };
    let bounds = [10.0, 20.0];
    sent(
        &sink_of(&client),
        Scope::of(org(1)),
        vec![a_metric(
            "http.duration",
            "ms",
            MetricData::Histogram {
                temporality: Temporality::Cumulative,
                points: vec![
                    a_histogram(10, &bounds, vec![10, 0, 0], 50.0),
                    a_histogram(20, &bounds, vec![10, 10, 0], 200.0),
                ],
            },
        )],
    )
    .await;
    let reader = ClickhouseMetricReader::new(client);
    let read = async |aggregation: Aggregation| {
        values_of(
            &reader
                .series(
                    org(1),
                    MetricKind::Histogram,
                    aggregation,
                    &charting("http.duration", aggregation, &[]),
                )
                .await
                .expect("a series set"),
            None,
        )
    };

    assert_eq!(read(Aggregation::P50).await, vec![(20, 15.0)]);
    assert_eq!(read(Aggregation::Avg).await, vec![(20, 15.0)]);
    assert_eq!(read(Aggregation::Rate).await, vec![(20, 1.0)]);
}

#[tokio::test]
async fn an_exemplar_sent_with_a_histogram_point_comes_back_with_its_trace_and_one_without_is_not_kept()
 {
    let Some(client) = prepared("ferristrace_metrics_exemplars").await else {
        return;
    };
    let trace = TraceId::try_from([7u8; 16].as_slice()).expect("sixteen bytes");
    let span = SpanId::try_from([3u8; 8].as_slice()).expect("eight bytes");
    let mut point = a_histogram(10, &[10.0], vec![1, 0], 4.0);
    point.exemplars = vec![
        Exemplar {
            time_unix_nanos: at(9),
            value: 4.0,
            trace_id: Some(trace),
            span_id: Some(span),
            attributes: labelled(&[("user.id", "u-1")]),
        },
        Exemplar {
            time_unix_nanos: at(11),
            value: 6.0,
            trace_id: None,
            span_id: None,
            attributes: BTreeMap::new(),
        },
    ];
    sent(
        &sink_of(&client),
        Scope::of(org(1)),
        vec![a_metric(
            "http.duration",
            "ms",
            MetricData::Histogram {
                temporality: Temporality::Delta,
                points: vec![point],
            },
        )],
    )
    .await;

    let set = ClickhouseMetricReader::new(client)
        .series(
            org(1),
            MetricKind::Histogram,
            Aggregation::P95,
            &charting("http.duration", Aggregation::P95, &[]),
        )
        .await
        .expect("a series set");

    assert_eq!(
        set.exemplars.len(),
        1,
        "an exemplar without a trace is not kept"
    );
    let traced = &set.exemplars[0];
    assert_eq!(traced.trace_id, Some(trace));
    assert_eq!(traced.span_id, Some(span));
    assert_eq!(traced.value, 4.0);
    assert_eq!(traced.attributes, labelled(&[("user.id", "u-1")]));
}

#[tokio::test]
async fn the_metrics_of_one_organisation_are_never_read_for_another_and_a_project_narrows() {
    let Some(client) = prepared("ferristrace_metrics_isolation").await else {
        return;
    };
    let sink = sink_of(&client);
    let gauge = |value: f64| {
        vec![a_metric(
            "queue.depth",
            "1",
            MetricData::Gauge(vec![a_point(5, value, "/")]),
        )]
    };
    sent(
        &sink,
        Scope::of(org(1)).in_project(Some(project(30))),
        gauge(1.0),
    )
    .await;
    sent(
        &sink,
        Scope::of(org(1)).in_project(Some(project(40))),
        gauge(2.0),
    )
    .await;
    sent(&sink, Scope::of(org(2)), gauge(100.0)).await;
    let reader = ClickhouseMetricReader::new(client);
    let summed = async |tenant: TenantId, project: Option<ProjectId>| {
        let mut query = charting("queue.depth", Aggregation::Sum, &[]);
        query.project = project;
        reader
            .series(tenant, MetricKind::Gauge, Aggregation::Sum, &query)
            .await
            .expect("a series set")
    };

    assert_eq!(values_of(&summed(org(1), None).await, None), vec![(5, 3.0)]);
    assert_eq!(
        values_of(&summed(org(1), Some(project(40))).await, None),
        vec![(5, 2.0)]
    );
    assert_eq!(
        values_of(&summed(org(2), None).await, None),
        vec![(5, 100.0)]
    );
    assert!(summed(org(3), None).await.series.is_empty());
    assert_eq!(
        reader
            .catalogue(org(1), None, Some(project(30)))
            .await
            .expect("the catalogue")
            .len(),
        1
    );
    assert!(
        reader
            .catalogue(org(1), None, Some(project(50)))
            .await
            .expect("the catalogue")
            .is_empty()
    );
    assert!(
        reader
            .catalogue(org(3), None, None)
            .await
            .expect("the catalogue")
            .is_empty()
    );
}

#[derive(Debug, clickhouse::Row, serde::Deserialize)]
struct Expiry {
    value: u32,
}

async fn expiry_of(client: &Client, table: &str, name: &str) -> u32 {
    client
        .query(&format!(
            "SELECT min(expire_at) AS value FROM {table} WHERE name = ?"
        ))
        .bind(name)
        .fetch_one::<Expiry>()
        .await
        .expect("reading the expiry")
        .value
}

#[tokio::test]
async fn every_metric_row_expires_after_the_retention_of_its_scope_or_the_default() {
    let Some(client) = prepared("ferristrace_metrics_expiry").await else {
        return;
    };
    let sink = sink_of(&client);
    let week = RetentionDays::try_from(7).expect("a week");
    let mut point = a_histogram(1, &[1.0], vec![1, 0], 1.0);
    point.exemplars = vec![Exemplar {
        time_unix_nanos: at(1),
        value: 1.0,
        trace_id: Some(TraceId::try_from([1u8; 16].as_slice()).expect("an id")),
        span_id: None,
        attributes: BTreeMap::new(),
    }];
    sent(
        &sink,
        Scope::of(org(1)).retained_for(Some(week)),
        vec![
            a_metric(
                "kept.a.week",
                "1",
                MetricData::Gauge(vec![a_point(1, 1.0, "/")]),
            ),
            a_metric(
                "kept.a.week.too",
                "1",
                MetricData::Histogram {
                    temporality: Temporality::Delta,
                    points: vec![point],
                },
            ),
        ],
    )
    .await;
    sent(
        &sink,
        Scope::of(org(1)),
        vec![a_metric(
            "kept.by.default",
            "1",
            MetricData::Gauge(vec![a_point(1, 1.0, "/")]),
        )],
    )
    .await;

    let week_end = NOW + 7 * DAY;
    assert_eq!(
        u64::from(expiry_of(&client, "metric_points", "kept.a.week").await),
        week_end
    );
    assert_eq!(
        u64::from(expiry_of(&client, "metric_histograms", "kept.a.week.too").await),
        week_end
    );
    assert_eq!(
        u64::from(expiry_of(&client, "metric_exemplars", "kept.a.week.too").await),
        week_end
    );
    assert_eq!(
        u64::from(expiry_of(&client, "metric_catalogue", "kept.a.week").await),
        week_end
    );
    assert_eq!(
        u64::from(expiry_of(&client, "metric_points", "kept.by.default").await),
        NOW + 30 * DAY
    );
}

#[tokio::test]
async fn an_opened_span_store_writes_and_reads_metrics_too() {
    let Some(url) = backend::clickhouse_url() else {
        return;
    };
    let database = "ferristrace_metrics_opened";
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
        .metrics
        .accept_metrics(
            Scope::of(org(1)),
            vec![a_metric(
                "queue.depth",
                "1",
                MetricData::Gauge(vec![a_point(1, 1.0, "/")]),
            )],
        )
        .await
        .expect("the sink accepts");
    store.metrics.flush().await.expect("the sink flushes");

    let listed = store
        .metric_reader
        .catalogue(org(1), None, None)
        .await
        .expect("the catalogue");
    assert_eq!(listed.len(), 1);
}

#[tokio::test]
async fn a_series_groups_by_a_resource_attribute_of_the_export() {
    use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
    use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
    use opentelemetry_proto::tonic::metrics::v1::metric::Data;
    use opentelemetry_proto::tonic::metrics::v1::number_data_point::Value as NumberValue;
    use opentelemetry_proto::tonic::metrics::v1::{
        Gauge, Metric as ProtoMetric, NumberDataPoint, ResourceMetrics, ScopeMetrics,
    };
    use opentelemetry_proto::tonic::resource::v1::Resource;

    let Some(client) = prepared("ferristrace_metrics_resource").await else {
        return;
    };
    let text = |key: &str, held: &str| KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(held.to_owned())),
        }),
        ..KeyValue::default()
    };
    let from_host = |host: &str, value: f64| ResourceMetrics {
        resource: Some(Resource {
            attributes: vec![text("service.name", "checkout"), text("host.name", host)],
            ..Resource::default()
        }),
        scope_metrics: vec![ScopeMetrics {
            metrics: vec![ProtoMetric {
                name: "queue.depth".to_owned(),
                unit: "1".to_owned(),
                data: Some(Data::Gauge(Gauge {
                    data_points: vec![NumberDataPoint {
                        time_unix_nano: at(5),
                        value: Some(NumberValue::AsDouble(value)),
                        ..NumberDataPoint::default()
                    }],
                })),
                ..ProtoMetric::default()
            }],
            ..ScopeMetrics::default()
        }],
        ..ResourceMetrics::default()
    };
    let decoded = ferristrace_otlp::metrics_of(ExportMetricsServiceRequest {
        resource_metrics: vec![from_host("web-1", 1.0), from_host("web-2", 2.0)],
    })
    .expect("a valid export");
    sent(&sink_of(&client), Scope::of(org(1)), decoded).await;

    let set = ClickhouseMetricReader::new(client)
        .series(
            org(1),
            MetricKind::Gauge,
            Aggregation::Avg,
            &charting("queue.depth", Aggregation::Avg, &["host.name"]),
        )
        .await
        .expect("a series set");

    assert_eq!(
        set.series
            .iter()
            .map(|series| (series.labels.clone(), series.points[5].1))
            .collect::<Vec<_>>(),
        vec![
            (labelled(&[("host.name", "web-1")]), Some(1.0)),
            (labelled(&[("host.name", "web-2")]), Some(2.0)),
        ]
    );
}
