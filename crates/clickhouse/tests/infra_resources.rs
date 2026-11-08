use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{
    BatchLimits, ClickhouseMetricReader, ClickhouseMetricSink, ClickhouseMigrator, ClickhouseSink,
};
use ferristrace_domain::{
    InfraFilter, InfraReader, MetricSink, PlacementName, Resource, ResourceKind, Scope, SpanSink,
    TenantId, Window,
};
use ferristrace_infra::FixedClock;
use ferristrace_otlp::{metrics_of, spans_of};
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
use opentelemetry_proto::tonic::metrics::v1::{
    Gauge, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, metric::Data, number_data_point,
};
use opentelemetry_proto::tonic::resource::v1::Resource as ProtoResource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as ProtoSpan};
use uuid::Uuid;

mod backend;

const SINK_NOW: u64 = 4_000_000_000;
const SECOND: u64 = 1_000_000_000;
const MINUTE: u64 = 60 * SECOND;
const NOW_SECS: u64 = 1_700_003_600;
const NOW: u64 = NOW_SECS * SECOND;
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

fn text(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(value.to_owned())),
        }),
        ..KeyValue::default()
    }
}

fn resource(pairs: &[(&str, &str)]) -> ProtoResource {
    ProtoResource {
        attributes: pairs.iter().map(|(key, value)| text(key, value)).collect(),
        ..ProtoResource::default()
    }
}

#[derive(Clone, Copy)]
enum Number {
    Int(i64),
    Double(f64),
}

type Sample<'a> = (u64, Number, &'a [(&'a str, &'a str)]);

fn gauge(name: &str, unit: &str, points: &[Sample<'_>]) -> Metric {
    Metric {
        name: name.to_owned(),
        unit: unit.to_owned(),
        data: Some(Data::Gauge(Gauge {
            data_points: points
                .iter()
                .map(|(ago, number, attributes)| NumberDataPoint {
                    time_unix_nano: NOW - ago,
                    value: Some(match number {
                        Number::Int(value) => number_data_point::Value::AsInt(*value),
                        Number::Double(value) => number_data_point::Value::AsDouble(*value),
                    }),
                    attributes: attributes
                        .iter()
                        .map(|(key, value)| text(key, value))
                        .collect(),
                    ..NumberDataPoint::default()
                })
                .collect(),
        })),
        ..Metric::default()
    }
}

fn reported(resource: ProtoResource, metrics: Vec<Metric>) -> ResourceMetrics {
    ResourceMetrics {
        resource: Some(resource),
        scope_metrics: vec![ScopeMetrics {
            metrics,
            ..ScopeMetrics::default()
        }],
        ..ResourceMetrics::default()
    }
}

fn limits() -> BatchLimits {
    BatchLimits {
        max_rows: 100_000,
        period: Duration::from_secs(60),
    }
}

const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

async fn sent(client: &Client, tenant: TenantId, metrics: Vec<ResourceMetrics>) {
    let metrics = metrics_of(ExportMetricsServiceRequest {
        resource_metrics: metrics,
    })
    .expect("a valid metric export");
    let sink = ClickhouseMetricSink::new(client, limits(), RETENTION, FixedClock::at(SINK_NOW));
    sink.accept_metrics(Scope::of(tenant), metrics)
        .await
        .expect("metrics accepted");
    sink.flush().await.expect("metrics flushed");
}

fn level(name: &str, ago: u64) -> Metric {
    gauge(name, "1", &[(ago, Number::Double(0.5), &[])])
}

fn mine() -> Vec<ResourceMetrics> {
    vec![
        reported(
            resource(&[("k8s.cluster.name", "prod-eu"), ("k8s.node.name", "n1")]),
            vec![level("k8s.node.cpu.utilization", MINUTE)],
        ),
        reported(
            resource(&[("k8s.cluster.name", "prod-eu"), ("k8s.node.name", "n2")]),
            vec![level("k8s.node.cpu.utilization", 0)],
        ),
        reported(
            resource(&[("k8s.cluster.name", "staging")]),
            vec![level("k8s.pod.phase", 30 * MINUTE)],
        ),
        reported(
            resource(&[
                ("host.name", "web-1"),
                ("cloud.provider", "aws"),
                ("cloud.region", "eu-west-1"),
                ("host.type", "m5.large"),
                ("deployment.environment.name", "prod"),
            ]),
            vec![level("system.cpu.utilization", 0)],
        ),
        reported(
            resource(&[("host.name", "web-1")]),
            vec![level("system.memory.utilization", 5 * MINUTE)],
        ),
        reported(
            resource(&[("container.name", "db"), ("container.id", "c0ffee")]),
            vec![
                gauge(
                    "container.cpu.utilization",
                    "%",
                    &[(2 * MINUTE, Number::Double(10.0), &[])],
                ),
                gauge(
                    "container.cpu.utilization",
                    "%",
                    &[(0, Number::Double(37.5), &[])],
                ),
                gauge(
                    "container.memory.usage.total",
                    "By",
                    &[(0, Number::Int(2_048), &[])],
                ),
            ],
        ),
        reported(
            resource(&[
                ("container.name", "sidecar"),
                ("k8s.pod.name", "mailer-1"),
                ("k8s.cluster.name", "prod-eu"),
            ]),
            vec![level("container.cpu.utilization", 0)],
        ),
        reported(
            resource(&[("service.name", "checkout")]),
            vec![level("http.server.requests", 0)],
        ),
    ]
}

fn theirs() -> Vec<ResourceMetrics> {
    vec![reported(
        resource(&[("host.name", "stranger-1"), ("k8s.cluster.name", "alien")]),
        vec![level("system.cpu.utilization", 0)],
    )]
}

async fn seeded(client: &Client) {
    sent(client, org(MINE), mine()).await;
    sent(client, org(THEIRS), theirs()).await;
}

fn the_hour() -> Window {
    Window::new(NOW - 10 * MINUTE, NOW + SECOND).expect("ten minutes")
}

fn named(text: &str) -> PlacementName {
    PlacementName::try_from(text).expect("a name")
}

async fn listed(client: &Client, tenant: TenantId, filter: &InfraFilter) -> Vec<Resource> {
    ClickhouseMetricReader::new(client.clone())
        .resources(tenant, the_hour(), filter)
        .await
        .expect("resources")
}

fn names(resources: &[Resource]) -> Vec<(&'static str, String)> {
    resources
        .iter()
        .map(|r| (r.kind.name(), r.name.to_string()))
        .collect()
}

fn one<'a>(resources: &'a [Resource], kind: ResourceKind, name: &str) -> &'a Resource {
    resources
        .iter()
        .find(|r| r.kind == kind && &*r.name == name)
        .unwrap_or_else(|| panic!("{kind:?} {name} in {resources:?}"))
}

#[tokio::test]
async fn clusters_hosts_and_standalone_containers_are_each_listed_once() {
    let Some(client) = prepared("ft_infra_kinds").await else {
        return;
    };
    seeded(&client).await;

    let all = listed(&client, org(MINE), &InfraFilter::default()).await;

    assert_eq!(
        names(&all),
        [
            ("cluster", "prod-eu".to_owned()),
            ("cluster", "staging".to_owned()),
            ("host", "web-1".to_owned()),
            ("container", "db".to_owned()),
        ],
        "{all:?}"
    );
}

#[tokio::test]
async fn a_container_inside_a_pod_is_not_a_container_resource() {
    let Some(client) = prepared("ft_infra_pod_container").await else {
        return;
    };
    seeded(&client).await;

    let all = listed(&client, org(MINE), &InfraFilter::default()).await;

    assert!(all.iter().all(|r| &*r.name != "sidecar"), "{all:?}");
}

#[tokio::test]
async fn a_resource_keeps_first_and_last_seen_across_many_reports() {
    let Some(client) = prepared("ft_infra_seen").await else {
        return;
    };
    seeded(&client).await;

    let all = listed(&client, org(MINE), &InfraFilter::default()).await;
    let prod = one(&all, ResourceKind::Cluster, "prod-eu");
    let staging = one(&all, ResourceKind::Cluster, "staging");
    let host = one(&all, ResourceKind::Host, "web-1");

    assert_eq!(prod.first_seen_unix_nanos, NOW - MINUTE);
    assert_eq!(prod.last_seen_unix_nanos, NOW);
    assert_eq!(staging.last_seen_unix_nanos, NOW - 30 * MINUTE);
    assert_eq!(host.first_seen_unix_nanos, NOW - 5 * MINUTE);
    assert_eq!(host.last_seen_unix_nanos, NOW);
}

#[tokio::test]
async fn cloud_attributes_and_environment_are_facets_of_the_host() {
    let Some(client) = prepared("ft_infra_facets").await else {
        return;
    };
    seeded(&client).await;

    let all = listed(&client, org(MINE), &InfraFilter::default()).await;
    let host = one(&all, ResourceKind::Host, "web-1");

    assert_eq!(&*host.provider, "aws");
    assert_eq!(&*host.region, "eu-west-1");
    assert_eq!(&*host.host_type, "m5.large");
    assert_eq!(&*host.environment, "prod");
}

#[tokio::test]
async fn a_container_carries_its_latest_cpu_and_memory() {
    let Some(client) = prepared("ft_infra_figures").await else {
        return;
    };
    seeded(&client).await;

    let all = listed(&client, org(MINE), &InfraFilter::default()).await;
    let db = one(&all, ResourceKind::Container, "db");

    assert_eq!(db.cpu_percent, Some(37.5));
    assert_eq!(db.memory_bytes, Some(2_048));
}

#[tokio::test]
async fn the_filters_narrow_by_kind_provider_region_and_cluster() {
    let Some(client) = prepared("ft_infra_filters").await else {
        return;
    };
    seeded(&client).await;
    let by = |filter: InfraFilter| {
        let client = client.clone();
        async move { names(&listed(&client, org(MINE), &filter).await) }
    };

    assert_eq!(
        by(InfraFilter {
            kind: Some(ResourceKind::Host),
            ..InfraFilter::default()
        })
        .await,
        [("host", "web-1".to_owned())]
    );
    assert_eq!(
        by(InfraFilter {
            provider: Some(named("aws")),
            ..InfraFilter::default()
        })
        .await,
        [("host", "web-1".to_owned())]
    );
    assert!(
        by(InfraFilter {
            region: Some(named("us-east-1")),
            ..InfraFilter::default()
        })
        .await
        .is_empty()
    );
    assert_eq!(
        by(InfraFilter {
            cluster: Some(named("staging")),
            ..InfraFilter::default()
        })
        .await,
        [("cluster", "staging".to_owned())]
    );
}

#[tokio::test]
async fn another_organisation_never_sees_these_resources() {
    let Some(client) = prepared("ft_infra_tenants").await else {
        return;
    };
    seeded(&client).await;

    let theirs = listed(&client, org(THEIRS), &InfraFilter::default()).await;

    assert_eq!(
        names(&theirs),
        [
            ("cluster", "alien".to_owned()),
            ("host", "stranger-1".to_owned())
        ]
    );
    assert!(
        listed(&client, org(3), &InfraFilter::default())
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_hostile_name_in_a_filter_is_bound_not_spliced() {
    let Some(client) = prepared("ft_infra_hostile").await else {
        return;
    };
    seeded(&client).await;
    let hostile = InfraFilter {
        cluster: PlacementName::try_from("prod-eu").ok(),
        ..InfraFilter::default()
    };

    assert_eq!(listed(&client, org(MINE), &hostile).await.len(), 1);
    assert!(PlacementName::try_from("x' OR '1'='1").is_err());
}

fn worked(service: &str, seed: u8, pairs: &[(&str, &str)]) -> ResourceSpans {
    let mut held = resource(pairs);
    held.attributes.push(text("service.name", service));
    ResourceSpans {
        resource: Some(held),
        scope_spans: vec![ScopeSpans {
            spans: vec![ProtoSpan {
                trace_id: vec![seed; 16],
                span_id: vec![seed; 8],
                name: "work".to_owned(),
                kind: 2,
                start_time_unix_nano: NOW - 2 * MINUTE,
                end_time_unix_nano: NOW - 2 * MINUTE + 10_000_000,
                ..ProtoSpan::default()
            }],
            ..ScopeSpans::default()
        }],
        ..ResourceSpans::default()
    }
}

async fn spanned(client: &Client, tenant: TenantId, spans: Vec<ResourceSpans>) {
    let spans = spans_of(ExportTraceServiceRequest {
        resource_spans: spans,
    })
    .expect("a valid trace export");
    let sink = ClickhouseSink::new(client, limits(), RETENTION, FixedClock::at(SINK_NOW));
    sink.accept(Scope::of(tenant), spans)
        .await
        .expect("spans accepted");
    sink.flush().await.expect("spans flushed");
}

#[tokio::test]
async fn a_resource_lists_the_services_whose_spans_ran_on_it() {
    let Some(client) = prepared("ft_infra_services").await else {
        return;
    };
    seeded(&client).await;
    spanned(
        &client,
        org(MINE),
        vec![
            worked(
                "checkout",
                1,
                &[("host.name", "web-1"), ("k8s.cluster.name", "prod-eu")],
            ),
            worked("mailer", 2, &[("host.name", "web-1")]),
            worked("api", 3, &[("container.name", "db")]),
            worked(
                "sidecar-app",
                4,
                &[("container.name", "x"), ("k8s.pod.name", "p-1")],
            ),
        ],
    )
    .await;
    spanned(
        &client,
        org(THEIRS),
        vec![worked("stranger-app", 9, &[("host.name", "web-1")])],
    )
    .await;

    let all = listed(&client, org(MINE), &InfraFilter::default()).await;
    let services = |kind, name: &str| -> Vec<String> {
        one(&all, kind, name)
            .services
            .iter()
            .map(ToString::to_string)
            .collect()
    };

    assert_eq!(
        services(ResourceKind::Host, "web-1"),
        ["checkout", "mailer"]
    );
    assert_eq!(services(ResourceKind::Cluster, "prod-eu"), ["checkout"]);
    assert_eq!(services(ResourceKind::Container, "db"), ["api"]);
    assert!(services(ResourceKind::Cluster, "staging").is_empty());
}
