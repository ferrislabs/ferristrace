use ferristrace_domain::{MetricData, MetricKind, Temporality};
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
use opentelemetry_proto::tonic::metrics::v1::exemplar::Value as ExemplarValue;
use opentelemetry_proto::tonic::metrics::v1::metric::Data;
use opentelemetry_proto::tonic::metrics::v1::number_data_point::Value as NumberValue;
use opentelemetry_proto::tonic::metrics::v1::{
    AggregationTemporality, Exemplar, ExponentialHistogram, ExponentialHistogramDataPoint, Gauge,
    Histogram, HistogramDataPoint, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, Sum,
    Summary, SummaryDataPoint,
};
use opentelemetry_proto::tonic::resource::v1::Resource;
use prost::Message;

use crate::metrics::{decode_metrics, metrics_of, skipped_was_reported};
use crate::{DecodeError, UNKNOWN_SERVICE};

fn text(key: &str, held: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(held.to_owned())),
        }),
        ..KeyValue::default()
    }
}

fn a_number(time: u64, value: NumberValue, route: &str) -> NumberDataPoint {
    NumberDataPoint {
        time_unix_nano: time,
        value: Some(value),
        attributes: vec![text("http.route", route)],
        ..NumberDataPoint::default()
    }
}

fn named(name: &str, data: Data) -> Metric {
    Metric {
        name: name.to_owned(),
        unit: "ms".to_owned(),
        description: "how long".to_owned(),
        data: Some(data),
        ..Metric::default()
    }
}

fn a_request(metrics: Vec<Metric>, service: Option<&str>) -> ExportMetricsServiceRequest {
    ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            resource: service.map(|service| Resource {
                attributes: vec![text("service.name", service)],
                ..Resource::default()
            }),
            scope_metrics: vec![ScopeMetrics {
                metrics,
                ..ScopeMetrics::default()
            }],
            ..ResourceMetrics::default()
        }],
    }
}

fn decoded(metrics: Vec<Metric>) -> Vec<ferristrace_domain::Metric> {
    decode_metrics(&a_request(metrics, Some("checkout")).encode_to_vec()).expect("a valid request")
}

fn an_exemplar(trace: Vec<u8>, span: Vec<u8>) -> Exemplar {
    Exemplar {
        time_unix_nano: 77,
        value: Some(ExemplarValue::AsInt(12)),
        trace_id: trace,
        span_id: span,
        filtered_attributes: vec![text("user.id", "u-1")],
    }
}

#[test]
fn a_gauge_keeps_its_name_unit_description_service_and_points() {
    let metrics = decoded(vec![named(
        "queue.depth",
        Data::Gauge(Gauge {
            data_points: vec![
                a_number(10, NumberValue::AsDouble(2.5), "/a"),
                a_number(20, NumberValue::AsInt(7), "/b"),
            ],
        }),
    )]);

    assert_eq!(metrics.len(), 1);
    let metric = &metrics[0];
    assert_eq!(&*metric.name, "queue.depth");
    assert_eq!(&*metric.unit, "ms");
    assert_eq!(&*metric.description, "how long");
    assert_eq!(&*metric.service_name, "checkout");
    assert_eq!(metric.data.kind(), MetricKind::Gauge);
    let MetricData::Gauge(points) = &metric.data else {
        panic!("a gauge");
    };
    assert_eq!(
        points
            .iter()
            .map(|point| (point.time_unix_nanos, point.value))
            .collect::<Vec<_>>(),
        vec![(10, 2.5), (20, 7.0)]
    );
    assert_eq!(
        points[1].attributes.get("http.route").map(|v| &**v),
        Some("/b")
    );
}

#[test]
fn a_sum_keeps_its_monotonic_flag_and_temporality() {
    let metrics = decoded(vec![
        named(
            "http.requests",
            Data::Sum(Sum {
                data_points: vec![a_number(10, NumberValue::AsInt(3), "/a")],
                aggregation_temporality: AggregationTemporality::Cumulative as i32,
                is_monotonic: true,
            }),
        ),
        named(
            "queue.changes",
            Data::Sum(Sum {
                data_points: vec![a_number(10, NumberValue::AsDouble(-1.0), "/a")],
                aggregation_temporality: AggregationTemporality::Delta as i32,
                is_monotonic: false,
            }),
        ),
        named(
            "odd",
            Data::Sum(Sum {
                data_points: vec![a_number(10, NumberValue::AsDouble(1.0), "/a")],
                aggregation_temporality: 42,
                is_monotonic: false,
            }),
        ),
    ]);

    let shapes = metrics
        .iter()
        .map(|metric| match &metric.data {
            MetricData::Sum {
                monotonic,
                temporality,
                points,
            } => (*monotonic, *temporality, points[0].value),
            other => panic!("a sum, not {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        shapes,
        vec![
            (true, Temporality::Cumulative, 3.0),
            (false, Temporality::Delta, -1.0),
            (false, Temporality::Unspecified, 1.0),
        ]
    );
}

#[test]
fn a_histogram_keeps_its_bounds_counts_sum_count_min_and_max() {
    let metrics = decoded(vec![named(
        "http.duration",
        Data::Histogram(Histogram {
            data_points: vec![
                HistogramDataPoint {
                    time_unix_nano: 30,
                    count: 6,
                    sum: Some(120.0),
                    bucket_counts: vec![1, 2, 3],
                    explicit_bounds: vec![10.0, 50.0],
                    min: Some(4.0),
                    max: Some(80.0),
                    attributes: vec![text("http.route", "/a")],
                    ..HistogramDataPoint::default()
                },
                HistogramDataPoint {
                    time_unix_nano: 40,
                    count: 1,
                    bucket_counts: vec![1],
                    ..HistogramDataPoint::default()
                },
            ],
            aggregation_temporality: AggregationTemporality::Delta as i32,
        }),
    )]);

    let MetricData::Histogram {
        temporality,
        points,
    } = &metrics[0].data
    else {
        panic!("a histogram");
    };
    assert_eq!(*temporality, Temporality::Delta);
    let point = &points[0];
    assert_eq!(point.time_unix_nanos, 30);
    assert_eq!(point.count, 6);
    assert_eq!(point.sum, 120.0);
    assert_eq!(point.bounds, vec![10.0, 50.0]);
    assert_eq!(point.counts, vec![1, 2, 3]);
    assert_eq!((point.min, point.max), (Some(4.0), Some(80.0)));
    assert_eq!(point.attributes.get("http.route").map(|v| &**v), Some("/a"));
    assert_eq!(
        (points[1].sum, points[1].min, points[1].max),
        (0.0, None, None)
    );
}

#[test]
fn exemplars_carry_their_trace_and_span_from_every_point_type() {
    let mut number = a_number(10, NumberValue::AsDouble(1.0), "/a");
    number.exemplars = vec![an_exemplar(vec![9u8; 16], vec![4u8; 8])];
    let histogram = HistogramDataPoint {
        time_unix_nano: 30,
        count: 1,
        bucket_counts: vec![1],
        exemplars: vec![an_exemplar(vec![8u8; 16], Vec::new())],
        ..HistogramDataPoint::default()
    };
    let metrics = decoded(vec![
        named(
            "g",
            Data::Gauge(Gauge {
                data_points: vec![number],
            }),
        ),
        named(
            "h",
            Data::Histogram(Histogram {
                data_points: vec![histogram],
                aggregation_temporality: AggregationTemporality::Delta as i32,
            }),
        ),
    ]);

    let MetricData::Gauge(points) = &metrics[0].data else {
        panic!("a gauge");
    };
    let exemplar = &points[0].exemplars[0];
    assert_eq!(exemplar.time_unix_nanos, 77);
    assert_eq!(exemplar.value, 12.0);
    assert_eq!(exemplar.trace_id.map(|id| *id.as_bytes()), Some([9u8; 16]));
    assert_eq!(exemplar.span_id.map(|id| *id.as_bytes()), Some([4u8; 8]));
    assert_eq!(
        exemplar.attributes.get("user.id").map(|v| &**v),
        Some("u-1")
    );
    let MetricData::Histogram { points, .. } = &metrics[1].data else {
        panic!("a histogram");
    };
    assert_eq!(
        points[0].exemplars[0].trace_id.map(|id| *id.as_bytes()),
        Some([8u8; 16])
    );
    assert_eq!(points[0].exemplars[0].span_id, None);
}

#[test]
fn an_exemplar_id_of_the_wrong_length_is_refused() {
    let mut number = a_number(10, NumberValue::AsDouble(1.0), "/a");
    number.exemplars = vec![an_exemplar(vec![1u8; 3], Vec::new())];
    let request = a_request(
        vec![named(
            "g",
            Data::Gauge(Gauge {
                data_points: vec![number],
            }),
        )],
        None,
    );

    assert!(matches!(
        decode_metrics(&request.encode_to_vec()),
        Err(DecodeError::Id(_))
    ));
}

#[test]
fn exponential_histograms_and_summaries_are_skipped_and_reported_while_the_rest_is_kept() {
    let metrics = decoded(vec![
        named(
            "latency.exponential",
            Data::ExponentialHistogram(ExponentialHistogram {
                data_points: vec![ExponentialHistogramDataPoint {
                    count: 3,
                    ..ExponentialHistogramDataPoint::default()
                }],
                aggregation_temporality: AggregationTemporality::Delta as i32,
            }),
        ),
        named(
            "latency.summary",
            Data::Summary(Summary {
                data_points: vec![SummaryDataPoint {
                    count: 3,
                    ..SummaryDataPoint::default()
                }],
            }),
        ),
        named(
            "queue.depth",
            Data::Gauge(Gauge {
                data_points: vec![a_number(10, NumberValue::AsDouble(1.0), "/a")],
            }),
        ),
    ]);

    assert_eq!(
        metrics.iter().map(|m| &*m.name).collect::<Vec<_>>(),
        vec!["queue.depth"]
    );
    assert!(skipped_was_reported());
}

#[test]
fn a_point_without_a_value_and_a_metric_without_points_are_dropped() {
    let metrics = decoded(vec![
        named(
            "empty",
            Data::Gauge(Gauge {
                data_points: vec![NumberDataPoint {
                    time_unix_nano: 10,
                    ..NumberDataPoint::default()
                }],
            }),
        ),
        Metric {
            name: "no.data".to_owned(),
            ..Metric::default()
        },
    ]);

    assert!(metrics.is_empty());
}

#[test]
fn a_resource_without_a_service_name_falls_back() {
    let request = a_request(
        vec![named(
            "g",
            Data::Gauge(Gauge {
                data_points: vec![a_number(1, NumberValue::AsDouble(1.0), "/")],
            }),
        )],
        None,
    );

    let metrics = metrics_of(request).expect("a valid request");

    assert_eq!(&*metrics[0].service_name, UNKNOWN_SERVICE);
}

#[test]
fn a_truncated_metrics_payload_is_refused() {
    assert!(matches!(
        decode_metrics(&[0xff, 0xff, 0xff]),
        Err(DecodeError::Protobuf(_))
    ));
}

#[test]
fn resource_attributes_reach_every_point_and_a_point_attribute_wins_a_conflict() {
    let mut request = a_request(
        vec![
            named(
                "g",
                Data::Gauge(Gauge {
                    data_points: vec![a_number(1, NumberValue::AsDouble(1.0), "/a")],
                }),
            ),
            named(
                "s",
                Data::Sum(Sum {
                    data_points: vec![a_number(1, NumberValue::AsDouble(1.0), "/a")],
                    aggregation_temporality: AggregationTemporality::Delta as i32,
                    is_monotonic: true,
                }),
            ),
            named(
                "h",
                Data::Histogram(Histogram {
                    data_points: vec![HistogramDataPoint {
                        time_unix_nano: 1,
                        count: 1,
                        bucket_counts: vec![1],
                        attributes: vec![text("http.route", "/h")],
                        ..HistogramDataPoint::default()
                    }],
                    aggregation_temporality: AggregationTemporality::Delta as i32,
                }),
            ),
        ],
        None,
    );
    request.resource_metrics[0].resource = Some(Resource {
        attributes: vec![
            text("service.name", "checkout"),
            text("host.name", "web-1"),
            text("http.route", "/from-the-resource"),
        ],
        ..Resource::default()
    });

    let metrics = metrics_of(request).expect("a valid request");

    let attributes = metrics
        .iter()
        .map(|metric| match &metric.data {
            MetricData::Gauge(points) | MetricData::Sum { points, .. } => {
                points[0].attributes.clone()
            }
            MetricData::Histogram { points, .. } => points[0].attributes.clone(),
        })
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 3);
    for (held, route) in attributes.iter().zip(["/a", "/a", "/h"]) {
        assert_eq!(held.get("host.name").map(|v| &**v), Some("web-1"));
        assert_eq!(held.get("service.name").map(|v| &**v), Some("checkout"));
        assert_eq!(held.get("http.route").map(|v| &**v), Some(route));
    }
}

#[test]
fn an_exemplar_without_a_trace_is_dropped_and_resource_attributes_do_not_reach_exemplars() {
    let mut number = a_number(10, NumberValue::AsDouble(1.0), "/a");
    number.exemplars = vec![
        an_exemplar(Vec::new(), vec![4u8; 8]),
        an_exemplar(vec![0u8; 16], Vec::new()),
        an_exemplar(vec![9u8; 16], Vec::new()),
    ];
    let request = a_request(
        vec![named(
            "g",
            Data::Gauge(Gauge {
                data_points: vec![number],
            }),
        )],
        Some("checkout"),
    );

    let metrics = metrics_of(request).expect("a valid request");

    let MetricData::Gauge(points) = &metrics[0].data else {
        panic!("a gauge");
    };
    assert_eq!(points[0].exemplars.len(), 1);
    let kept = &points[0].exemplars[0];
    assert_eq!(kept.trace_id.map(|id| *id.as_bytes()), Some([9u8; 16]));
    assert_eq!(kept.attributes.get("service.name"), None);
}
