use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Request, StatusCode};
use clickhouse::Client;
use ferristrace_api::{AppState, SessionCookie, explore_router, log_router, metric_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_clickhouse::{
    BatchLimits, ClickhouseLogReader, ClickhouseLogSink, ClickhouseMetricReader,
    ClickhouseMetricSink, ClickhouseMigrator, ClickhouseReader, ClickhouseSink, MIGRATIONS,
};
use ferristrace_domain::{
    AccountId, Agent, Aggregation, Buckets, LiveSession, LogQuery, LogReader, LogSink, Membership,
    Memberships, MetricKind, MetricQuery, MetricReader, MetricSink, NoPhotos, OrgRole, Placement,
    PlacementName, PlacementReader, ProjectId, ProvisionPolicy, Scope, SeenPod, SeenWorkload,
    ServiceFilter, Session, SessionError, SessionId, SessionRoll, SessionStore, SessionToken,
    SpanReader, SpanSink, TenantId, TraceFilter, UnixSeconds, WhoamiError, Window, Workload,
    WorkloadKind,
};
use ferristrace_infra::FixedClock;
use ferristrace_otlp::{logs_of, metrics_of, spans_of};
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
use opentelemetry_proto::tonic::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use opentelemetry_proto::tonic::metrics::v1::{
    Gauge, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, metric::Data, number_data_point,
};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as ProtoSpan};
use tower::ServiceExt;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const BASE: u64 = 1_700_000_000_000_000_000;
const SECOND: u64 = 1_000_000_000;
const MILLI: u64 = 1_000_000;
const HOUR: u64 = 3_600 * SECOND;
const TOKEN: &str = "ft_ss_placement";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const PLACEMENT_VERSION: u32 = 9;

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

fn the_hour() -> Window {
    Window::new(BASE, BASE + HOUR).expect("an hour")
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

struct Sender {
    seed: u8,
    service: &'static str,
    cluster: &'static str,
    namespace: &'static str,
    owners: &'static [(&'static str, &'static str)],
    pod: &'static str,
    node: &'static str,
}

const CHECKOUT_1: Sender = Sender {
    seed: 1,
    service: "checkout",
    cluster: "prod",
    namespace: "shop",
    owners: &[("k8s.deployment.name", "checkout")],
    pod: "checkout-7d9f-1",
    node: "node-a",
};

const CHECKOUT_2: Sender = Sender {
    seed: 2,
    service: "checkout",
    cluster: "prod",
    namespace: "shop",
    owners: &[("k8s.deployment.name", "checkout")],
    pod: "checkout-7d9f-2",
    node: "node-b",
};

const BACKUP: Sender = Sender {
    seed: 3,
    service: "backup",
    cluster: "prod",
    namespace: "ops",
    owners: &[
        ("k8s.cronjob.name", "backup"),
        ("k8s.job.name", "backup-28472"),
    ],
    pod: "backup-28472-x9",
    node: "node-a",
};

const POSTGRES: Sender = Sender {
    seed: 4,
    service: "postgres",
    cluster: "staging",
    namespace: "shop",
    owners: &[("k8s.statefulset.name", "postgres")],
    pod: "postgres-0",
    node: "node-c",
};

const STRANGER: Sender = Sender {
    seed: 9,
    service: "checkout",
    cluster: "theirs",
    namespace: "shop",
    owners: &[("k8s.deployment.name", "checkout")],
    pod: "checkout-theirs",
    node: "node-z",
};

const EVERY_POD: [&Sender; 4] = [&CHECKOUT_1, &CHECKOUT_2, &BACKUP, &POSTGRES];

impl Sender {
    fn resource(&self) -> Resource {
        let mut attributes = vec![
            text("service.name", self.service),
            text("k8s.cluster.name", self.cluster),
            text("k8s.namespace.name", self.namespace),
            text("k8s.pod.name", self.pod),
            text("k8s.node.name", self.node),
        ];
        attributes.extend(self.owners.iter().map(|(key, value)| text(key, value)));
        Resource {
            attributes,
            ..Resource::default()
        }
    }

    fn at(&self) -> u64 {
        BASE + u64::from(self.seed) * SECOND
    }

    fn spans(&self) -> ResourceSpans {
        ResourceSpans {
            resource: Some(self.resource()),
            scope_spans: vec![ScopeSpans {
                spans: vec![ProtoSpan {
                    trace_id: vec![self.seed; 16],
                    span_id: vec![self.seed; 8],
                    name: "work".to_owned(),
                    kind: 2,
                    start_time_unix_nano: self.at(),
                    end_time_unix_nano: self.at() + 10 * MILLI,
                    ..ProtoSpan::default()
                }],
                ..ScopeSpans::default()
            }],
            ..ResourceSpans::default()
        }
    }

    fn logs(&self) -> ResourceLogs {
        ResourceLogs {
            resource: Some(self.resource()),
            scope_logs: vec![ScopeLogs {
                log_records: (0..u64::from(self.seed))
                    .map(|n| LogRecord {
                        time_unix_nano: self.at() + n * MILLI,
                        severity_number: 17,
                        body: Some(AnyValue {
                            value: Some(Value::StringValue(format!("{} {n}", self.pod))),
                        }),
                        attributes: vec![text("attempt", &n.to_string())],
                        ..LogRecord::default()
                    })
                    .collect(),
                ..ScopeLogs::default()
            }],
            ..ResourceLogs::default()
        }
    }

    fn metrics(&self) -> ResourceMetrics {
        ResourceMetrics {
            resource: Some(self.resource()),
            scope_metrics: vec![ScopeMetrics {
                metrics: vec![Metric {
                    name: "queue.depth".to_owned(),
                    unit: "1".to_owned(),
                    data: Some(Data::Gauge(Gauge {
                        data_points: vec![NumberDataPoint {
                            time_unix_nano: self.at(),
                            value: Some(number_data_point::Value::AsDouble(
                                f64::from(self.seed) * 10.0,
                            )),
                            ..NumberDataPoint::default()
                        }],
                    })),
                    ..Metric::default()
                }],
                ..ScopeMetrics::default()
            }],
            ..ResourceMetrics::default()
        }
    }
}

fn limits() -> BatchLimits {
    BatchLimits {
        max_rows: 100_000,
        period: Duration::from_secs(60),
    }
}

const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

async fn sent(client: &Client, tenant: TenantId, pods: &[&Sender]) {
    let scope = Scope::of(tenant);
    let spans = spans_of(ExportTraceServiceRequest {
        resource_spans: pods.iter().map(|pod| pod.spans()).collect(),
    })
    .expect("a valid trace export");
    let logs = logs_of(ExportLogsServiceRequest {
        resource_logs: pods.iter().map(|pod| pod.logs()).collect(),
    })
    .expect("a valid log export");
    let metrics = metrics_of(ExportMetricsServiceRequest {
        resource_metrics: pods.iter().map(|pod| pod.metrics()).collect(),
    })
    .expect("a valid metric export");

    let span_sink = ClickhouseSink::new(client, limits(), RETENTION, FixedClock::at(NOW));
    span_sink
        .accept(scope, spans)
        .await
        .expect("spans accepted");
    span_sink.flush().await.expect("spans flushed");
    let log_sink = ClickhouseLogSink::new(client, limits(), RETENTION, FixedClock::at(NOW));
    log_sink
        .accept_logs(scope, logs)
        .await
        .expect("logs accepted");
    log_sink.flush().await.expect("logs flushed");
    let metric_sink = ClickhouseMetricSink::new(client, limits(), RETENTION, FixedClock::at(NOW));
    metric_sink
        .accept_metrics(scope, metrics)
        .await
        .expect("metrics accepted");
    metric_sink.flush().await.expect("metrics flushed");
}

fn named(text: &str) -> PlacementName {
    PlacementName::try_from(text).expect("a placement name")
}

fn on_pod(pod: &str) -> Placement {
    Placement {
        pod: Some(named(pod)),
        ..Placement::default()
    }
}

fn on_node(node: &str) -> Placement {
    Placement {
        node: Some(named(node)),
        ..Placement::default()
    }
}

fn of_workload(workload: &str) -> Placement {
    Placement {
        workload: Some(Workload::try_from(workload).expect("a workload")),
        ..Placement::default()
    }
}

fn in_namespace(namespace: &str) -> Placement {
    Placement {
        namespace: Some(named(namespace)),
        ..Placement::default()
    }
}

fn in_cluster(cluster: &str) -> Placement {
    Placement {
        cluster: Some(named(cluster)),
        ..Placement::default()
    }
}

async fn traced(reader: &ClickhouseReader, tenant: TenantId, placement: Placement) -> Vec<u8> {
    let mut filter = TraceFilter::over(the_hour());
    filter.placement = placement;
    let mut seeds: Vec<u8> = reader
        .traces(tenant, &filter)
        .await
        .expect("the traces read")
        .into_iter()
        .map(|summary| summary.trace_id.as_bytes()[0])
        .collect();
    seeds.sort_unstable();
    seeds
}

async fn logged(reader: &ClickhouseLogReader, tenant: TenantId, placement: Placement) -> u64 {
    let mut query = LogQuery::over(the_hour());
    query.placement = placement;
    let page = reader.search(tenant, &query).await.expect("the logs read");
    let counted: u64 = reader
        .histogram(tenant, &query, Buckets::default())
        .await
        .expect("the histogram reads")
        .iter()
        .map(ferristrace_domain::LogBucket::total)
        .sum();
    assert_eq!(page.records.len() as u64, counted);
    counted
}

async fn gauged(reader: &ClickhouseMetricReader, tenant: TenantId, placement: Placement) -> f64 {
    let mut query = MetricQuery::over(the_hour(), "queue.depth");
    query.placement = placement;
    reader
        .series(tenant, MetricKind::Gauge, Aggregation::Sum, &query)
        .await
        .expect("the series reads")
        .series
        .iter()
        .flat_map(|series| series.points.iter().filter_map(|(_, value)| *value))
        .sum()
}

#[tokio::test]
async fn spans_logs_and_metrics_are_found_by_every_part_of_their_placement() {
    let Some(client) = prepared("ferristrace_placement_filters").await else {
        return;
    };
    sent(&client, org(MINE), &EVERY_POD).await;
    let spans = ClickhouseReader::new(client.clone());
    let logs = ClickhouseLogReader::new(client.clone());
    let metrics = ClickhouseMetricReader::new(client);

    let cases: [(Placement, Vec<u8>); 10] = [
        (on_pod("checkout-7d9f-1"), vec![1]),
        (on_node("node-a"), vec![1, 3]),
        (of_workload("checkout"), vec![1, 2]),
        (of_workload("deployment:checkout"), vec![1, 2]),
        (of_workload("statefulset:checkout"), vec![]),
        (of_workload("backup"), vec![3]),
        (of_workload("cronjob:backup"), vec![3]),
        (of_workload("job:backup-28472"), vec![3]),
        (in_namespace("shop"), vec![1, 2, 4]),
        (in_cluster("staging"), vec![4]),
    ];
    for (placement, seeds) in cases {
        let logs_expected: u64 = seeds.iter().map(|seed| u64::from(*seed)).sum();
        let gauge_expected: f64 = seeds.iter().map(|seed| f64::from(*seed) * 10.0).sum();

        assert_eq!(
            traced(&spans, org(MINE), placement.clone()).await,
            seeds,
            "{placement:?}"
        );
        assert_eq!(
            logged(&logs, org(MINE), placement.clone()).await,
            logs_expected,
            "{placement:?}"
        );
        let gauge = gauged(&metrics, org(MINE), placement.clone()).await;
        assert!(
            (gauge - gauge_expected).abs() < 1e-9,
            "{placement:?}: {gauge}"
        );
    }
    assert_eq!(
        traced(&spans, org(MINE), Placement::default()).await,
        vec![1, 2, 3, 4]
    );
}

#[tokio::test]
async fn services_operations_and_series_narrow_to_the_placement() {
    let Some(client) = prepared("ferristrace_placement_services").await else {
        return;
    };
    sent(&client, org(MINE), &EVERY_POD).await;
    let reader = ClickhouseReader::new(client);
    let mut on_a = ServiceFilter::over(the_hour());
    on_a.placement = on_node("node-a");
    let mut checkout_1 = ServiceFilter::over(the_hour());
    checkout_1.placement = Placement {
        namespace: Some(named("shop")),
        workload: Some(Workload::try_from("checkout").expect("a workload")),
        pod: Some(named("checkout-7d9f-1")),
        ..Placement::default()
    };

    let mut services: Vec<(String, u64)> = reader
        .services(org(MINE), &on_a)
        .await
        .expect("the services read")
        .into_iter()
        .map(|summary| (summary.name.into_string(), summary.requests))
        .collect();
    services.sort();
    let operations = reader
        .operations(org(MINE), "checkout", &checkout_1)
        .await
        .expect("the operations read");
    let series = reader
        .series(org(MINE), None, &on_a)
        .await
        .expect("the series reads");
    let one_pod = reader
        .series(org(MINE), Some("checkout"), &checkout_1)
        .await
        .expect("the series reads");

    assert_eq!(
        services,
        vec![("backup".to_owned(), 1), ("checkout".to_owned(), 1)]
    );
    assert_eq!(operations.len(), 1);
    assert_eq!((&*operations[0].name, operations[0].requests), ("work", 1));
    assert_eq!(series.requests, 2);
    assert_eq!(one_pod.requests, 1);
}

#[tokio::test]
async fn a_metric_grouped_by_pod_shows_only_the_pods_of_the_placement() {
    let Some(client) = prepared("ferristrace_placement_grouped").await else {
        return;
    };
    sent(&client, org(MINE), &EVERY_POD).await;
    let mut query = MetricQuery::over(the_hour(), "queue.depth");
    query.group_by = vec!["k8s.pod.name".into()];
    query.placement = on_node("node-a");

    let set = ClickhouseMetricReader::new(client)
        .series(org(MINE), MetricKind::Gauge, Aggregation::Max, &query)
        .await
        .expect("the series reads");

    let mut pods: Vec<&str> = set
        .series
        .iter()
        .filter_map(|series| series.labels.get("k8s.pod.name").map(|pod| &**pod))
        .collect();
    pods.sort_unstable();
    assert_eq!(pods, vec!["backup-28472-x9", "checkout-7d9f-1"]);
}

#[tokio::test]
async fn rows_written_before_the_placement_columns_existed_are_still_found() {
    let Some(client) = prepared("ferristrace_placement_old_rows").await else {
        return;
    };
    let migrator = ClickhouseMigrator::new(client.clone());
    migrator
        .down_to(PLACEMENT_VERSION - 1)
        .await
        .expect("the placement columns are taken away");
    sent(&client, org(MINE), &EVERY_POD).await;
    let applied = migrator
        .up()
        .await
        .expect("the placement columns come back");
    let spans = ClickhouseReader::new(client.clone());
    let logs = ClickhouseLogReader::new(client.clone());
    let metrics = ClickhouseMetricReader::new(client);

    let after_placement: Vec<u32> = MIGRATIONS
        .iter()
        .map(|migration| migration.version)
        .filter(|version| *version >= PLACEMENT_VERSION)
        .collect();
    assert_eq!(applied, after_placement);
    assert_eq!(
        traced(&spans, org(MINE), on_pod("checkout-7d9f-2")).await,
        vec![2]
    );
    assert_eq!(
        traced(&spans, org(MINE), of_workload("backup")).await,
        vec![3]
    );
    assert_eq!(logged(&logs, org(MINE), on_node("node-a")).await, 1 + 3);
    let gauge = gauged(&metrics, org(MINE), of_workload("statefulset:postgres")).await;
    assert!((gauge - 40.0).abs() < 1e-9, "{gauge}");
    let seen = spans
        .placements(org(MINE), the_hour(), None)
        .await
        .expect("the placements read");
    assert_eq!(
        seen.nodes,
        ["node-a", "node-b", "node-c"].map(Box::<str>::from)
    );
}

#[tokio::test]
async fn a_placement_never_reaches_into_another_organisation() {
    let Some(client) = prepared("ferristrace_placement_isolation").await else {
        return;
    };
    sent(&client, org(MINE), &[&CHECKOUT_1]).await;
    sent(&client, org(THEIRS), &[&STRANGER]).await;
    let spans = ClickhouseReader::new(client.clone());
    let logs = ClickhouseLogReader::new(client.clone());
    let metrics = ClickhouseMetricReader::new(client);

    assert!(
        traced(&spans, org(MINE), on_pod("checkout-theirs"))
            .await
            .is_empty()
    );
    assert_eq!(logged(&logs, org(MINE), on_node("node-z")).await, 0);
    assert!(
        gauged(&metrics, org(MINE), in_cluster("theirs"))
            .await
            .abs()
            < 1e-9
    );
    assert_eq!(
        traced(&spans, org(THEIRS), of_workload("checkout")).await,
        vec![9]
    );
    let seen = spans
        .placements(org(MINE), the_hour(), None)
        .await
        .expect("the placements read");
    assert_eq!(seen.clusters, [Box::<str>::from("prod")]);
    assert_eq!(seen.nodes, [Box::<str>::from("node-a")]);
}

#[tokio::test]
async fn the_placements_seen_in_a_window_list_every_cluster_namespace_workload_pod_and_node() {
    let Some(client) = prepared("ferristrace_placement_listing").await else {
        return;
    };
    sent(&client, org(MINE), &EVERY_POD).await;
    let reader = ClickhouseReader::new(client);

    let seen = reader
        .placements(org(MINE), the_hour(), None)
        .await
        .expect("the placements read");
    let elsewhere = reader
        .placements(
            org(MINE),
            the_hour(),
            Some(ProjectId::new(Uuid::from_u128(77))),
        )
        .await
        .expect("the placements read");
    let before = reader
        .placements(
            org(MINE),
            Window::new(BASE - HOUR, BASE).expect("an hour before"),
            None,
        )
        .await
        .expect("the placements read");

    assert_eq!(seen.clusters, ["prod", "staging"].map(Box::<str>::from));
    assert_eq!(seen.namespaces, ["ops", "shop"].map(Box::<str>::from));
    assert_eq!(
        seen.nodes,
        ["node-a", "node-b", "node-c"].map(Box::<str>::from)
    );
    let workload = |kind, name: &str, namespace: &str| SeenWorkload {
        kind,
        name: name.into(),
        namespace: namespace.into(),
    };
    assert_eq!(
        seen.workloads,
        vec![
            workload(WorkloadKind::CronJob, "backup", "ops"),
            workload(WorkloadKind::Job, "backup-28472", "ops"),
            workload(WorkloadKind::Deployment, "checkout", "shop"),
            workload(WorkloadKind::StatefulSet, "postgres", "shop"),
        ]
    );
    let pod = |name: &str, namespace: &str, node: &str| SeenPod {
        name: name.into(),
        namespace: namespace.into(),
        node: node.into(),
    };
    assert_eq!(
        seen.pods,
        vec![
            pod("backup-28472-x9", "ops", "node-a"),
            pod("checkout-7d9f-1", "shop", "node-a"),
            pod("checkout-7d9f-2", "shop", "node-b"),
            pod("postgres-0", "shop", "node-c"),
        ]
    );
    assert!(elsewhere.clusters.is_empty() && elsewhere.pods.is_empty());
    assert!(before.clusters.is_empty() && before.workloads.is_empty());
}

struct OneSession;

impl SessionStore for OneSession {
    async fn issue(
        &self,
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Ok(SessionToken::from_plaintext(TOKEN))
    }

    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.exposed() != TOKEN {
            return Err(SessionError::Unknown);
        }
        Ok(Session {
            account: AccountId::new(Uuid::from_u128(9)),
            email: ferristrace_domain::Email::try_from("person@example.com").expect("an address"),
            name: None,
            photo: None,
            org: None,
            expires_at: UnixSeconds::new(0),
        })
    }

    async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
        Ok(())
    }
}

impl SessionRoll for OneSession {
    async fn live(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<Vec<LiveSession>, SessionError> {
        Ok(Vec::new())
    }

    async fn revoke_one(&self, _account: AccountId, _id: SessionId) -> Result<bool, SessionError> {
        Ok(false)
    }

    async fn revoke_others(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<u64, SessionError> {
        Ok(0)
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

struct Placing;

impl Stack for Placing {
    type Repos = Fakes;
    type Clock = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = ClickhouseReader;
    type IssueStore = Inert;
    type LogStore = ClickhouseLogReader;
    type MetricStore = ClickhouseMetricReader;
    type Meter = Inert;
    type Oidc = Inert;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

fn routes(client: &Client) -> Router {
    let adapters = Adapters {
        clock: Inert::default(),
        mailer: Inert::default(),
        webhooks: Inert::default(),
        photos: NoPhotos,
        meter: Inert::default(),
        span_store: Some(ClickhouseReader::new(client.clone())),
        issue_store: None,
        log_store: Some(ClickhouseLogReader::new(client.clone())),
        metric_store: Some(ClickhouseMetricReader::new(client.clone())),
        oidc: None,
        provision: ProvisionPolicy::default(),
    };
    let state = AppState::<Placing>::new(
        Arc::new(Ferristrace::new(Fakes, adapters)),
        SessionCookie::default(),
    );
    explore_router(state.clone())
        .merge(log_router(state.clone()))
        .merge(metric_router(state))
}

async fn asked(client: &Client, uri: &str) -> serde_json::Value {
    let request = Request::builder()
        .uri(uri)
        .header("x-requested-with", "fetch")
        .header(COOKIE, format!("ft_session={TOKEN}"))
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
async fn spec_l3_1_traces_logs_and_metric_series_filtered_by_pod_node_or_workload_return_only_that_placement()
 {
    let Some(client) = prepared("ferristrace_placement_spec_l3_1").await else {
        return;
    };
    sent(&client, org(MINE), &EVERY_POD).await;
    let org = Uuid::from_u128(MINE);
    let from = BASE / MILLI;
    let to = (BASE + HOUR) / MILLI;
    let window = format!("from={from}&to={to}");

    let by_pod = asked(
        &client,
        &format!("/api/orgs/{org}/traces?{window}&pod=checkout-7d9f-2"),
    )
    .await;
    let by_node = asked(
        &client,
        &format!("/api/orgs/{org}/logs?{window}&node=node-a"),
    )
    .await;
    let by_workload = asked(
        &client,
        &format!(
            "/api/orgs/{org}/metrics/queue.depth/series?{window}&agg=max\
             &group_by=k8s.pod.name&workload=deployment:checkout"
        ),
    )
    .await;
    let picked = asked(&client, &format!("/api/orgs/{org}/placements?{window}")).await;

    let traces = by_pod["traces"].as_array().expect("the traces");
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0]["trace_id"], "02".repeat(16));
    let records = by_node["records"].as_array().expect("the records");
    assert_eq!(records.len(), 1 + 3);
    assert!(
        records
            .iter()
            .all(|record| record["resource"]["k8s.node.name"] == "node-a"),
        "{records:?}"
    );
    let mut pods: Vec<&str> = by_workload["series"]
        .as_array()
        .expect("the series")
        .iter()
        .filter_map(|series| series["labels"]["k8s.pod.name"].as_str())
        .collect();
    pods.sort_unstable();
    assert_eq!(pods, vec!["checkout-7d9f-1", "checkout-7d9f-2"]);
    assert_eq!(
        picked["nodes"],
        serde_json::json!(["node-a", "node-b", "node-c"])
    );
    assert_eq!(
        picked["pods"][0],
        serde_json::json!({"name": "backup-28472-x9", "namespace": "ops", "node": "node-a"})
    );
}
