use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use clickhouse::Client;
use ferristrace_api::{AppState, cluster_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Bearers, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_clickhouse::{
    BatchLimits, ClickhouseMetricReader, ClickhouseMetricSink, ClickhouseMigrator, ClickhouseSink,
};
use ferristrace_domain::{
    AccountId, ClusterFilter, ClusterReader, ClusterSnapshot, Membership, Memberships, MetricSink,
    NoPhotos, OrgRole, PlacementName, Pod, PodPhase, PodReason, Scope, Session, SessionError,
    SessionStore, SessionToken, SpanSink, TenantId, UnixSeconds, WhoamiError, Window, WorkloadKind,
};
use ferristrace_infra::FixedClock;
use ferristrace_otlp::{metrics_of, spans_of};
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
use opentelemetry_proto::tonic::metrics::v1::{
    Gauge, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, metric::Data, number_data_point,
};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as ProtoSpan};
use tower::ServiceExt;
use uuid::Uuid;

mod backend;

const SINK_NOW: u64 = 4_000_000_000;
const SECOND: u64 = 1_000_000_000;
const MINUTE: u64 = 60 * SECOND;
const MILLI: u64 = 1_000_000;
const NOW_SECS: u64 = 1_700_003_600;
const NOW: u64 = NOW_SECS * SECOND;
const TOKEN: &str = "ft.person.cluster";
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

fn resource(pairs: &[(&str, &str)]) -> Resource {
    Resource {
        attributes: pairs.iter().map(|(key, value)| text(key, value)).collect(),
        ..Resource::default()
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

fn reported(resource: Resource, metrics: Vec<Metric>) -> ResourceMetrics {
    ResourceMetrics {
        resource: Some(resource),
        scope_metrics: vec![ScopeMetrics {
            metrics,
            ..ScopeMetrics::default()
        }],
        ..ResourceMetrics::default()
    }
}

struct PodAt {
    cluster: &'static str,
    namespace: &'static str,
    deployment: &'static str,
    pod: &'static str,
    node: &'static str,
}

impl PodAt {
    fn resource(&self) -> Resource {
        let mut pairs = vec![
            ("k8s.cluster.name", self.cluster),
            ("k8s.namespace.name", self.namespace),
            ("k8s.pod.name", self.pod),
            ("k8s.pod.uid", self.pod),
        ];
        if !self.node.is_empty() {
            pairs.push(("k8s.node.name", self.node));
        }
        if !self.deployment.is_empty() {
            pairs.push(("k8s.deployment.name", self.deployment));
            pairs.push(("k8s.replicaset.name", self.deployment));
        }
        resource(&pairs)
    }

    fn container(&self, container: &str) -> Resource {
        let mut held = self.resource();
        held.attributes.push(text("k8s.container.name", container));
        held
    }

    fn cluster_receiver(
        &self,
        ago: u64,
        phase: i64,
        restarts: &[(u64, i64)],
        ready: i64,
    ) -> Vec<ResourceMetrics> {
        let restarted: Vec<Sample<'_>> = restarts
            .iter()
            .map(|(at, count)| (*at, Number::Int(*count), &[][..]))
            .collect();
        vec![
            reported(
                self.resource(),
                vec![gauge(
                    "k8s.pod.phase",
                    "1",
                    &[(ago, Number::Int(phase), &[])],
                )],
            ),
            reported(
                self.container("app"),
                vec![
                    gauge("k8s.container.restarts", "{restart}", &restarted),
                    gauge(
                        "k8s.container.ready",
                        "1",
                        &[(ago, Number::Int(ready), &[])],
                    ),
                ],
            ),
        ]
    }

    fn kubelet(&self, ago: u64, cores: f64, bytes: i64) -> ResourceMetrics {
        reported(
            resource(&[
                ("k8s.cluster.name", self.cluster),
                ("k8s.namespace.name", self.namespace),
                ("k8s.pod.name", self.pod),
                ("k8s.pod.uid", self.pod),
            ]),
            vec![
                gauge(
                    "k8s.pod.cpu.usage",
                    "{cpu}",
                    &[(ago, Number::Double(cores), &[])],
                ),
                gauge(
                    "k8s.pod.memory.working_set",
                    "By",
                    &[(ago, Number::Int(bytes), &[])],
                ),
            ],
        )
    }

    fn spans(&self, service: &str, seed: u8) -> ResourceSpans {
        let mut held = self.resource();
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
                    end_time_unix_nano: NOW - 2 * MINUTE + 10 * MILLI,
                    ..ProtoSpan::default()
                }],
                ..ScopeSpans::default()
            }],
            ..ResourceSpans::default()
        }
    }
}

const CHECKOUT: PodAt = PodAt {
    cluster: "prod",
    namespace: "shop",
    deployment: "checkout",
    pod: "checkout-7d9f-1",
    node: "node-a",
};

const MAILER: PodAt = PodAt {
    cluster: "prod",
    namespace: "shop",
    deployment: "mailer",
    pod: "mailer-5c4b-1",
    node: "node-a",
};

const PENDING: PodAt = PodAt {
    cluster: "prod",
    namespace: "shop",
    deployment: "checkout",
    pod: "checkout-7d9f-2",
    node: "",
};

const WORKER: PodAt = PodAt {
    cluster: "prod",
    namespace: "jobs",
    deployment: "worker",
    pod: "worker-1",
    node: "node-b",
};

const STALE: PodAt = PodAt {
    cluster: "prod",
    namespace: "shop",
    deployment: "checkout",
    pod: "checkout-old-1",
    node: "node-a",
};

const STAGING: PodAt = PodAt {
    cluster: "staging",
    namespace: "shop",
    deployment: "checkout",
    pod: "checkout-stage-1",
    node: "node-s",
};

const STRANGER: PodAt = PodAt {
    cluster: "prod",
    namespace: "shop",
    deployment: "checkout",
    pod: "checkout-theirs",
    node: "node-z",
};

fn node(cluster: &str, name: &str, metrics: Vec<Metric>) -> ResourceMetrics {
    reported(
        resource(&[("k8s.cluster.name", cluster), ("k8s.node.name", name)]),
        metrics,
    )
}

fn deployment(
    cluster: &str,
    namespace: &str,
    name: &str,
    desired: i64,
    available: i64,
) -> ResourceMetrics {
    reported(
        resource(&[
            ("k8s.cluster.name", cluster),
            ("k8s.namespace.name", namespace),
            ("k8s.deployment.name", name),
            ("k8s.deployment.uid", name),
        ]),
        vec![
            gauge(
                "k8s.deployment.desired",
                "{pod}",
                &[(MINUTE, Number::Int(desired), &[])],
            ),
            gauge(
                "k8s.deployment.available",
                "{pod}",
                &[(MINUTE, Number::Int(available), &[])],
            ),
        ],
    )
}

fn mine() -> Vec<ResourceMetrics> {
    let mut sent = Vec::new();
    sent.extend(CHECKOUT.cluster_receiver(MINUTE, 2, &[(9 * MINUTE, 1), (MINUTE, 1)], 1));
    sent.push(CHECKOUT.kubelet(MINUTE, 0.25, 200_000_000));
    sent.extend(MAILER.cluster_receiver(
        MINUTE,
        2,
        &[(8 * MINUTE, 3), (4 * MINUTE, 5), (MINUTE, 7)],
        0,
    ));
    sent.push(MAILER.kubelet(MINUTE, 0.05, 500_000_000));
    sent.extend(PENDING.cluster_receiver(MINUTE, 1, &[], 0));
    sent.extend(WORKER.cluster_receiver(MINUTE, 2, &[(MINUTE, 0)], 1));
    sent.extend(STALE.cluster_receiver(11 * MINUTE, 4, &[(11 * MINUTE, 9)], 0));
    sent.extend(STAGING.cluster_receiver(MINUTE, 2, &[(MINUTE, 0)], 1));
    sent.push(node(
        "prod",
        "node-a",
        vec![
            gauge(
                "k8s.node.condition_ready",
                "1",
                &[(MINUTE, Number::Int(1), &[])],
            ),
            gauge(
                "k8s.node.cpu.usage",
                "{cpu}",
                &[(MINUTE, Number::Double(1.5), &[])],
            ),
            gauge(
                "k8s.node.memory.working_set",
                "By",
                &[(MINUTE, Number::Int(4_000_000_000), &[])],
            ),
            gauge(
                "k8s.node.memory.available",
                "By",
                &[(MINUTE, Number::Int(8_000_000_000), &[])],
            ),
        ],
    ));
    sent.push(node(
        "prod",
        "node-b",
        vec![gauge(
            "k8s.node.condition",
            "1",
            &[
                (MINUTE, Number::Int(0), &[("condition", "Ready")]),
                (MINUTE, Number::Int(0), &[("condition", "MemoryPressure")]),
            ],
        )],
    ));
    sent.push(node(
        "prod",
        "node-gone",
        vec![gauge(
            "k8s.node.condition_ready",
            "1",
            &[(20 * MINUTE, Number::Int(1), &[])],
        )],
    ));
    sent.push(deployment("prod", "shop", "checkout", 3, 1));
    sent.push(deployment("prod", "shop", "mailer", 1, 0));
    sent
}

fn limits() -> BatchLimits {
    BatchLimits {
        max_rows: 100_000,
        period: Duration::from_secs(60),
    }
}

const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

async fn sent(
    client: &Client,
    tenant: TenantId,
    metrics: Vec<ResourceMetrics>,
    spans: Vec<ResourceSpans>,
) {
    let scope = Scope::of(tenant);
    let metrics = metrics_of(ExportMetricsServiceRequest {
        resource_metrics: metrics,
    })
    .expect("a valid metric export");
    let metric_sink =
        ClickhouseMetricSink::new(client, limits(), RETENTION, FixedClock::at(SINK_NOW));
    metric_sink
        .accept_metrics(scope, metrics)
        .await
        .expect("metrics accepted");
    metric_sink.flush().await.expect("metrics flushed");
    if spans.is_empty() {
        return;
    }
    let spans = spans_of(ExportTraceServiceRequest {
        resource_spans: spans,
    })
    .expect("a valid trace export");
    let span_sink = ClickhouseSink::new(client, limits(), RETENTION, FixedClock::at(SINK_NOW));
    span_sink
        .accept(scope, spans)
        .await
        .expect("spans accepted");
    span_sink.flush().await.expect("spans flushed");
}

async fn seeded(client: &Client) {
    sent(
        client,
        org(MINE),
        mine(),
        vec![
            CHECKOUT.spans("checkout", 1),
            CHECKOUT.spans("checkout-events", 2),
            MAILER.spans("mailer", 3),
        ],
    )
    .await;
    sent(
        client,
        org(THEIRS),
        STRANGER.cluster_receiver(MINUTE, 2, &[(MINUTE, 0)], 1),
        vec![STRANGER.spans("checkout", 9)],
    )
    .await;
}

fn the_hour() -> Window {
    Window::new(NOW - 60 * MINUTE, NOW + SECOND).expect("an hour")
}

fn named(text: &str) -> PlacementName {
    PlacementName::try_from(text).expect("a name")
}

async fn looked(client: &Client, tenant: TenantId, filter: &ClusterFilter) -> ClusterSnapshot {
    ClickhouseMetricReader::new(client.clone())
        .snapshot(tenant, the_hour(), filter)
        .await
        .expect("a snapshot")
        .settled(filter)
}

fn pod<'a>(snapshot: &'a ClusterSnapshot, name: &str) -> &'a Pod {
    snapshot
        .pods
        .iter()
        .find(|pod| &*pod.name == name)
        .unwrap_or_else(|| panic!("{name} in {:?}", snapshot.pods))
}

fn pod_names(snapshot: &ClusterSnapshot) -> Vec<&str> {
    snapshot.pods.iter().map(|pod| &*pod.name).collect()
}

#[tokio::test]
async fn the_snapshot_holds_the_nodes_workloads_and_pods_the_receivers_reported() {
    let Some(client) = prepared("ferristrace_cluster_snapshot").await else {
        return;
    };
    seeded(&client).await;

    let snapshot = looked(&client, org(MINE), &ClusterFilter::default()).await;

    assert_eq!(
        snapshot.clusters,
        vec![Box::from("prod"), Box::from("staging")]
    );
    assert_eq!(
        snapshot.namespaces,
        vec![Box::from("jobs"), Box::from("shop")]
    );
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .map(|node| (&*node.name, node.ready, node.pods))
            .collect::<Vec<_>>(),
        vec![
            ("node-a", Some(true), 2),
            ("node-b", Some(false), 1),
            ("node-s", None, 1)
        ]
    );
    let node_a = &snapshot.nodes[0];
    assert_eq!(node_a.cpu_cores, Some(1.5));
    assert_eq!(node_a.memory_bytes, Some(4_000_000_000));
    assert_eq!(node_a.memory_available, Some(8_000_000_000));
    assert_eq!(
        snapshot
            .workloads
            .iter()
            .map(|workload| (
                &*workload.cluster,
                &*workload.namespace,
                workload.kind,
                &*workload.name,
                workload.desired,
                workload.ready,
                workload.pods
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "prod",
                "jobs",
                WorkloadKind::Deployment,
                "worker",
                None,
                None,
                1
            ),
            (
                "prod",
                "shop",
                WorkloadKind::Deployment,
                "checkout",
                Some(3),
                Some(1),
                2
            ),
            (
                "prod",
                "shop",
                WorkloadKind::Deployment,
                "mailer",
                Some(1),
                Some(0),
                1
            ),
            (
                "staging",
                "shop",
                WorkloadKind::Deployment,
                "checkout",
                None,
                None,
                1
            ),
        ]
    );
    assert_eq!(
        pod_names(&snapshot),
        vec![
            "worker-1",
            "checkout-7d9f-1",
            "checkout-7d9f-2",
            "checkout-stage-1",
            "mailer-5c4b-1"
        ]
    );
}

#[tokio::test]
async fn each_pod_carries_its_phase_restarts_usage_services_and_judgement() {
    let Some(client) = prepared("ferristrace_cluster_pods").await else {
        return;
    };
    seeded(&client).await;

    let snapshot = looked(&client, org(MINE), &ClusterFilter::default()).await;

    let checkout = pod(&snapshot, "checkout-7d9f-1");
    assert_eq!(checkout.phase, Some(PodPhase::Running));
    assert_eq!(checkout.node.as_deref(), Some("node-a"));
    assert_eq!(
        checkout.workload,
        Some((WorkloadKind::Deployment, Box::from("checkout")))
    );
    assert_eq!(checkout.restarts_in_window, 0);
    assert_eq!(checkout.restarts_total, 1);
    assert_eq!(checkout.cpu_cores, Some(0.25));
    assert_eq!(checkout.memory_bytes, Some(200_000_000));
    assert_eq!(checkout.ready, Some(true));
    assert_eq!(
        checkout.services,
        vec![Box::from("checkout"), Box::from("checkout-events")]
    );
    assert!(!checkout.unhealthy);
    assert_eq!(checkout.reason, None);

    let mailer = pod(&snapshot, "mailer-5c4b-1");
    assert_eq!(mailer.restarts_in_window, 4);
    assert_eq!(mailer.restarts_total, 7);
    assert_eq!(mailer.ready, Some(false));
    assert!(mailer.unhealthy);
    assert_eq!(mailer.reason, Some(PodReason::Restarting));
    assert_eq!(mailer.services, vec![Box::from("mailer")]);

    let pending = pod(&snapshot, "checkout-7d9f-2");
    assert_eq!(pending.node, None);
    assert_eq!(pending.reason, Some(PodReason::Phase(PodPhase::Pending)));
    assert_eq!(
        pod(&snapshot, "worker-1").reason,
        Some(PodReason::NodeNotReady)
    );
    assert_eq!(snapshot.seen_ms, Some((NOW - MINUTE) / MILLI));
}

#[tokio::test]
async fn the_same_workload_in_two_clusters_stays_two_workloads() {
    let Some(client) = prepared("ferristrace_cluster_two_clusters").await else {
        return;
    };
    seeded(&client).await;
    sent(
        &client,
        org(MINE),
        vec![deployment("staging", "shop", "checkout", 2, 2)],
        Vec::new(),
    )
    .await;

    let snapshot = looked(&client, org(MINE), &ClusterFilter::default()).await;

    let checkouts: Vec<(&str, Option<u64>, Option<u64>, u64)> = snapshot
        .workloads
        .iter()
        .filter(|workload| &*workload.name == "checkout")
        .map(|workload| {
            (
                &*workload.cluster,
                workload.desired,
                workload.ready,
                workload.pods,
            )
        })
        .collect();
    assert_eq!(
        checkouts,
        vec![
            ("prod", Some(3), Some(1), 2),
            ("staging", Some(2), Some(2), 1)
        ]
    );
}

#[tokio::test]
async fn nothing_older_than_ten_minutes_is_shown() {
    let Some(client) = prepared("ferristrace_cluster_fresh").await else {
        return;
    };
    seeded(&client).await;

    let snapshot = looked(&client, org(MINE), &ClusterFilter::default()).await;

    assert!(!pod_names(&snapshot).contains(&"checkout-old-1"));
    assert!(snapshot.nodes.iter().all(|node| &*node.name != "node-gone"));
}

#[tokio::test]
async fn a_namespace_or_a_cluster_narrows_the_snapshot() {
    let Some(client) = prepared("ferristrace_cluster_filters").await else {
        return;
    };
    seeded(&client).await;

    let jobs = looked(
        &client,
        org(MINE),
        &ClusterFilter {
            namespace: Some(named("jobs")),
            ..ClusterFilter::default()
        },
    )
    .await;
    let staging = looked(
        &client,
        org(MINE),
        &ClusterFilter {
            cluster: Some(named("staging")),
            ..ClusterFilter::default()
        },
    )
    .await;

    assert_eq!(pod_names(&jobs), vec!["worker-1"]);
    assert_eq!(
        jobs.nodes
            .iter()
            .map(|node| &*node.name)
            .collect::<Vec<_>>(),
        vec!["node-b"]
    );
    assert_eq!(jobs.nodes[0].ready, Some(false));
    assert!(
        jobs.workloads
            .iter()
            .all(|workload| &*workload.namespace == "jobs")
    );
    assert_eq!(jobs.clusters, vec![Box::from("prod"), Box::from("staging")]);

    assert_eq!(pod_names(&staging), vec!["checkout-stage-1"]);
    assert_eq!(
        staging
            .nodes
            .iter()
            .map(|node| &*node.name)
            .collect::<Vec<_>>(),
        vec!["node-s"]
    );
    assert!(
        staging
            .workloads
            .iter()
            .all(|workload| workload.desired.is_none())
    );
    assert_eq!(staging.namespaces, vec![Box::from("shop")]);
    assert_eq!(
        staging.clusters,
        vec![Box::from("prod"), Box::from("staging")]
    );
}

#[tokio::test]
async fn an_organisation_sees_only_its_own_cluster() {
    let Some(client) = prepared("ferristrace_cluster_isolation").await else {
        return;
    };
    seeded(&client).await;

    let theirs = looked(&client, org(THEIRS), &ClusterFilter::default()).await;
    let mine = looked(&client, org(MINE), &ClusterFilter::default()).await;
    let nobody = looked(&client, org(99), &ClusterFilter::default()).await;

    assert_eq!(pod_names(&theirs), vec!["checkout-theirs"]);
    assert_eq!(theirs.pods[0].services, vec![Box::from("checkout")]);
    assert!(!pod_names(&mine).contains(&"checkout-theirs"));
    assert!(mine.nodes.iter().all(|node| &*node.name != "node-z"));
    assert_eq!(nobody, ClusterSnapshot::default());
}

struct OneSession;

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
            Ok(Session {
                account: AccountId::new(Uuid::from_u128(9)),
                email: ferristrace_domain::Email::try_from("person@example.com")
                    .expect("an address"),
                name: None,
                photo: None,
                org: None,
                expires_at: UnixSeconds::new(0),
            })
        } else {
            Err(SessionError::Unknown)
        }
    }
}

struct MemberOfMine;

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: org(MINE),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

struct Fakes;

impl Untransacted for Fakes {}

impl RepoFor<domain::Sessions> for Fakes {
    type Repo = OneSession;

    fn build(&self, (): &()) -> Self::Repo {
        OneSession
    }
}

impl RepoFor<domain::Memberships> for Fakes {
    type Repo = MemberOfMine;

    fn build(&self, (): &()) -> Self::Repo {
        MemberOfMine
    }
}

impl RepoFor<domain::ApiKeys> for Fakes {
    type Repo = Inert;

    fn build(&self, (): &()) -> Self::Repo {
        Inert::default()
    }
}

struct Clustered;

impl Stack for Clustered {
    type Repos = Fakes;
    type Clock = FixedClock;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = ClickhouseMetricReader;
    type Meter = Inert;
    type Users = Bearers;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

fn routes(client: &Client) -> Router {
    let adapters = Adapters {
        clock: FixedClock::at(NOW_SECS),
        mailer: Inert::default(),
        webhooks: Inert::default(),
        photos: NoPhotos,
        meter: Inert::default(),
        span_store: None,
        issue_store: None,
        log_store: None,
        metric_store: Some(ClickhouseMetricReader::new(client.clone())),
        users: Some(Bearers),
    };
    cluster_router(AppState::<Clustered>::new(Arc::new(Ferristrace::new(
        Fakes, adapters,
    ))))
}

async fn asked(client: &Client, uri: &str) -> serde_json::Value {
    let request = Request::builder()
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .expect("a request");
    let response = routes(client).oneshot(request).await.expect("a response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    assert_eq!(status, StatusCode::OK, "{uri}: {body:?}");
    serde_json::from_slice(&body).expect("json")
}

#[tokio::test]
async fn spec_l3_2_pods_reported_by_the_collector_appear_by_node_and_a_crash_looping_pod_is_unhealthy()
 {
    let Some(client) = prepared("ferristrace_cluster_spec_l3_2").await else {
        return;
    };
    seeded(&client).await;
    let org = Uuid::from_u128(MINE);

    let seen = asked(&client, &format!("/api/orgs/{org}/cluster?cluster=prod")).await;

    let on_node_a: Vec<&str> = seen["pods"]
        .as_array()
        .expect("the pods")
        .iter()
        .filter(|pod| pod["node"] == "node-a")
        .filter_map(|pod| pod["name"].as_str())
        .collect();
    assert_eq!(on_node_a, vec!["checkout-7d9f-1", "mailer-5c4b-1"]);
    let node_a = &seen["nodes"][0];
    assert_eq!(node_a["name"], "node-a");
    assert_eq!(node_a["ready"], true);
    assert_eq!(node_a["pods"], 2);
    let mailer = seen["pods"]
        .as_array()
        .expect("the pods")
        .iter()
        .find(|pod| pod["name"] == "mailer-5c4b-1")
        .expect("the crash-looping pod");
    assert_eq!(
        *mailer,
        serde_json::json!({
            "name": "mailer-5c4b-1",
            "namespace": "shop",
            "cluster": "prod",
            "workload": {"kind": "deployment", "name": "mailer"},
            "node": "node-a",
            "phase": "Running",
            "ready": false,
            "restarts": 4,
            "restarts_total": 7,
            "cpu": 0.05,
            "memory": 500_000_000u64,
            "services": ["mailer"],
            "unhealthy": true,
            "reason": "restarting",
        })
    );
    let checkout = seen["pods"]
        .as_array()
        .expect("the pods")
        .iter()
        .find(|pod| pod["name"] == "checkout-7d9f-1")
        .expect("the healthy pod");
    assert_eq!(checkout["phase"], "Running");
    assert_eq!(checkout["restarts"], 0);
    assert_eq!(checkout["cpu"], 0.25);
    assert_eq!(checkout["memory"], 200_000_000u64);
    assert_eq!(checkout["unhealthy"], false);
    assert_eq!(checkout["reason"], serde_json::Value::Null);
    assert!(
        seen["pods"]
            .as_array()
            .expect("the pods")
            .iter()
            .all(|pod| pod["cluster"] == "prod")
    );
}
