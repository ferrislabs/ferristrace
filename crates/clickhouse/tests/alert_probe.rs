use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use clickhouse::Client;
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{AlertLedger, Couriered, Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, SignalReaders, Stack};
use ferristrace_clickhouse::{
    BatchLimits, ClickhouseLogReader, ClickhouseLogSink, ClickhouseMetricReader,
    ClickhouseMetricSink, ClickhouseMigrator, ClickhouseReader, ClickhouseSink,
};
use ferristrace_domain::{
    AccountId, Agent, Aggregation, AlertRules, AlertSeverity, AlertStatus, Assigned, Clock,
    Comparison, Condition, Created, Definition, DefinitionParts, Labels, LiveSession, LogRecord,
    LogSignal, LogSink, Membership, Memberships, Metric, MetricData, MetricPoint, MetricSignal,
    MetricSink, NoDataPolicy, NoPhotos, Observation, Project, ProjectId, ProjectName, Projects,
    ProvisionPolicy, RuleDraft, RuleName, Scope, Session, SessionError, SessionId, SessionRoll,
    SessionStore, SessionToken, Severity, Signal, SignalProbe, Span, SpanId, SpanKind, SpanSink,
    SpanStatus, StoreError, TeamId, TenantId, Tick, TraceId, TraceMeasure, TraceSignal,
    UnixSeconds, WhoamiError, Window,
};
use ferristrace_infra::FixedClock;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const BASE_SECONDS: u64 = 1_700_000_000;
const SECOND: u64 = 1_000_000_000;
const MILLI: u64 = 1_000_000;
const DAY: u64 = 86_400;

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

fn limits() -> BatchLimits {
    BatchLimits {
        max_rows: 100_000,
        period: Duration::from_secs(60),
    }
}

fn nanos(second: u64) -> u64 {
    (BASE_SECONDS + second) * SECOND
}

fn window(from_second: u64, to_second: u64) -> Window {
    Window::new(nanos(from_second), nanos(to_second)).expect("a window")
}

fn a_span(seed: u32, service: &str, name: &str, second: u64, took_ms: u64, failed: bool) -> Span {
    let id = seed.to_be_bytes();
    let mut trace = [5u8; 16];
    trace[12..].copy_from_slice(&id);
    let mut span = [6u8; 8];
    span[4..].copy_from_slice(&id);
    Span {
        trace_id: TraceId::try_from(trace.as_slice()).expect("sixteen bytes"),
        span_id: SpanId::try_from(span.as_slice()).expect("eight bytes"),
        parent: None,
        name: name.into(),
        service_name: service.into(),
        kind: SpanKind::Server,
        status: if failed {
            SpanStatus::Error
        } else {
            SpanStatus::Ok
        },
        start_unix_nanos: nanos(second),
        end_unix_nanos: nanos(second) + took_ms * MILLI,
        attributes: BTreeMap::new(),
        events: Vec::new(),
    }
}

async fn stored(client: &Client, scope: Scope, spans: Vec<Span>) {
    let sink = ClickhouseSink::new(
        client,
        limits(),
        Duration::from_secs(7 * DAY),
        FixedClock::at(NOW),
    );
    sink.accept(scope, spans).await.expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
}

fn traces(service: Option<&str>, operation: Option<&str>, measure: TraceMeasure) -> Signal {
    Signal::Traces(TraceSignal {
        service: service.map(Into::into),
        operation: operation.map(Into::into),
        environment: None,
        measure,
    })
}

async fn observed<P: SignalProbe>(
    probe: &P,
    tenant: TenantId,
    signal: &Signal,
    over: Window,
) -> Vec<Observation> {
    probe
        .observe(tenant, None, signal, over)
        .await
        .expect("the probe reads")
}

fn only(found: &[Observation]) -> Option<f64> {
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].labels.is_empty());
    found[0].value
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn the_probe_reads_the_trace_measures_of_one_organisation() {
    let Some(client) = prepared("ferristrace_alert_probe_traces").await else {
        return;
    };
    let mut spans = Vec::new();
    for seed in 0..20u32 {
        let name = if seed < 10 { "POST /pay" } else { "GET /cart" };
        spans.push(a_span(
            seed,
            "checkout",
            name,
            u64::from(seed),
            u64::from(seed + 1) * 10,
            seed % 10 == 0,
        ));
    }
    spans.push(a_span(100, "billing", "charge", 5, 900, true));
    stored(&client, Scope::of(org(1)), spans).await;
    stored(
        &client,
        Scope::of(org(2)),
        (200..250)
            .map(|seed| a_span(seed, "checkout", "POST /pay", 10, 5, true))
            .collect(),
    )
    .await;
    let reader = ClickhouseReader::new(client);
    let probe = SignalReaders::<_, Inert, Inert>::new(Some(&reader), None, None);
    let minute = window(0, 60);

    let count = only(
        &observed(
            &probe,
            org(1),
            &traces(Some("checkout"), None, TraceMeasure::Count),
            minute,
        )
        .await,
    );
    let rate = only(
        &observed(
            &probe,
            org(1),
            &traces(Some("checkout"), None, TraceMeasure::ErrorRate),
            minute,
        )
        .await,
    );
    let p95 = only(
        &observed(
            &probe,
            org(1),
            &traces(Some("checkout"), None, TraceMeasure::P95),
            minute,
        )
        .await,
    );
    let paying = only(
        &observed(
            &probe,
            org(1),
            &traces(Some("checkout"), Some("POST /pay"), TraceMeasure::ErrorRate),
            minute,
        )
        .await,
    );
    let everything = only(
        &observed(
            &probe,
            org(1),
            &traces(None, None, TraceMeasure::Count),
            minute,
        )
        .await,
    );
    let p99_everywhere = only(
        &observed(
            &probe,
            org(1),
            &traces(None, None, TraceMeasure::P99),
            minute,
        )
        .await,
    );
    let quiet = only(
        &observed(
            &probe,
            org(1),
            &traces(Some("checkout"), None, TraceMeasure::ErrorRate),
            window(120, 180),
        )
        .await,
    );
    let quiet_count = only(
        &observed(
            &probe,
            org(1),
            &traces(Some("checkout"), None, TraceMeasure::Count),
            window(120, 180),
        )
        .await,
    );

    assert_eq!(
        count,
        Some(20.0),
        "the other organisation's fifty spans are not counted"
    );
    assert_eq!(
        rate,
        Some(10.0),
        "two errors in twenty requests is ten percent"
    );
    let p95 = p95.expect("a p95");
    assert!((180.0..=200.0).contains(&p95), "{p95} ms");
    assert_eq!(paying, Some(10.0), "one error in the ten payments");
    assert_eq!(everything, Some(21.0));
    let p99 = p99_everywhere.expect("a p99");
    assert!(
        p99 > 200.0,
        "the slow billing span counts across services: {p99} ms"
    );
    assert_eq!(quiet, None, "no request is no data for a rate");
    assert_eq!(quiet_count, Some(0.0), "no request is a count of zero");
}

#[tokio::test]
async fn the_probe_counts_the_logs_a_query_matches() {
    let Some(client) = prepared("ferristrace_alert_probe_logs").await else {
        return;
    };
    let sink = ClickhouseLogSink::new(
        &client,
        limits(),
        Duration::from_secs(30 * DAY),
        FixedClock::at(NOW),
    );
    let log = |second: u64, service: &str, severity: Severity, body: &str| LogRecord {
        time_unix_nanos: nanos(second),
        observed_unix_nanos: nanos(second) + 1,
        severity,
        severity_number: 0,
        service_name: service.into(),
        body: body.into(),
        attributes: BTreeMap::from([("region".into(), "eu".into())]),
        resource: BTreeMap::new(),
        trace_id: None,
        span_id: None,
    };
    sink.accept_logs(
        Scope::of(org(1)),
        vec![
            log(1, "checkout", Severity::Error, "payment declined"),
            log(2, "checkout", Severity::Error, "payment timeout"),
            log(3, "checkout", Severity::Fatal, "payment crashed"),
            log(4, "checkout", Severity::Info, "payment accepted"),
            log(5, "billing", Severity::Error, "invoice failed"),
            log(500, "checkout", Severity::Error, "much later"),
        ],
    )
    .await
    .expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
    let reader = ClickhouseLogReader::new(client);
    let probe = SignalReaders::<Inert, _, Inert>::new(None, Some(&reader), None);
    let errors = Signal::Logs(LogSignal {
        services: vec!["checkout".into()],
        severities: vec![Severity::Error, Severity::Fatal],
        text: Some("payment".into()),
        attributes: vec![("region".into(), "eu".into())],
    });

    let counted = only(&observed(&probe, org(1), &errors, window(0, 60)).await);
    let elsewhere = only(&observed(&probe, org(2), &errors, window(0, 60)).await);

    assert_eq!(counted, Some(3.0));
    assert_eq!(elsewhere, Some(0.0));
}

type Seen = (BTreeMap<Box<str>, Box<str>>, Option<f64>);

#[tokio::test]
async fn the_probe_reads_a_metric_per_label_set_and_through_a_filter() {
    let Some(client) = prepared("ferristrace_alert_probe_metrics").await else {
        return;
    };
    let sink = ClickhouseMetricSink::new(
        &client,
        BatchLimits {
            max_rows: 1,
            period: Duration::from_secs(60),
        },
        Duration::from_secs(30 * DAY),
        FixedClock::at(NOW),
    );
    let point = |second: u64, value: f64, queue: &str| MetricPoint {
        time_unix_nanos: nanos(second),
        value,
        attributes: BTreeMap::from([
            ("queue".into(), queue.into()),
            ("env".into(), "prod".into()),
        ]),
        exemplars: Vec::new(),
    };
    sink.accept_metrics(
        Scope::of(org(1)),
        vec![Metric {
            name: "queue.depth".into(),
            unit: "{message}".into(),
            description: "depth".into(),
            service_name: "worker".into(),
            data: MetricData::Gauge(vec![
                point(5, 10.0, "mail"),
                point(35, 40.0, "mail"),
                point(10, 3.0, "billing"),
                point(40, 2.0, "billing"),
            ]),
        }],
    )
    .await
    .expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
    let reader = ClickhouseMetricReader::new(client);
    let probe = SignalReaders::<Inert, Inert, _>::new(None, None, Some(&reader));
    let grouped = Signal::Metrics(MetricSignal {
        name: "queue.depth".into(),
        aggregation: Aggregation::Max,
        group_by: vec!["queue".into()],
        filter: BTreeMap::from([("env".into(), "prod".into())]),
    });
    let filtered = Signal::Metrics(MetricSignal {
        name: "queue.depth".into(),
        aggregation: Aggregation::Max,
        group_by: Vec::new(),
        filter: BTreeMap::from([("queue".into(), "billing".into())]),
    });
    let unknown = Signal::Metrics(MetricSignal {
        name: "never.sent".into(),
        aggregation: Aggregation::Avg,
        group_by: Vec::new(),
        filter: BTreeMap::new(),
    });

    let mut per_queue: Vec<Seen> = observed(&probe, org(1), &grouped, window(0, 60))
        .await
        .into_iter()
        .map(|observation| (observation.labels.pairs().clone(), observation.value))
        .collect();
    per_queue.sort_by(|one, other| one.0.cmp(&other.0));
    let billing = only(&observed(&probe, org(1), &filtered, window(0, 60)).await);
    let nothing = observed(&probe, org(1), &unknown, window(0, 60)).await;
    let theirs = observed(&probe, org(2), &grouped, window(0, 60)).await;

    assert_eq!(
        per_queue,
        vec![
            (
                BTreeMap::from([("queue".into(), "billing".into())]),
                Some(3.0)
            ),
            (
                BTreeMap::from([("queue".into(), "mail".into())]),
                Some(40.0)
            ),
        ]
    );
    assert_eq!(billing, Some(3.0));
    assert!(nothing.is_empty(), "a metric never seen is no data");
    assert!(theirs.is_empty());
}

#[derive(Debug, Clone, Default)]
struct Moving(Arc<AtomicU64>);

impl Moving {
    fn to(&self, second: u64) {
        self.0.store(BASE_SECONDS + second, Ordering::SeqCst);
    }
}

impl Clock for Moving {
    fn now(&self) -> UnixSeconds {
        UnixSeconds::new(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Clone)]
struct Nobody;

impl SessionStore for Nobody {
    async fn issue(
        &self,
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Err(SessionError::Unknown)
    }

    async fn resolve(&self, _token: &SessionToken) -> Result<Session, SessionError> {
        Err(SessionError::Unknown)
    }

    async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
        Ok(())
    }
}

impl SessionRoll for Nobody {
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

impl Memberships for Nobody {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(Vec::new())
    }
}

impl Projects for Nobody {
    async fn of(&self, _tenant: TenantId) -> Result<Vec<Project>, StoreError> {
        Ok(Vec::new())
    }

    async fn create(&self, _org: TenantId, _name: &ProjectName) -> Result<Created, StoreError> {
        Ok(Created::Done)
    }

    async fn rename(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _name: &ProjectName,
    ) -> Result<ferristrace_domain::Renamed, StoreError> {
        Ok(ferristrace_domain::Renamed::Gone)
    }

    async fn forget(&self, _org: TenantId, _project: ProjectId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn assign(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _team: Option<TeamId>,
    ) -> Result<Assigned, StoreError> {
        Ok(Assigned::Gone)
    }
}

struct Ledgered(AlertLedger);

impl Untransacted for Ledgered {}

macro_rules! repo {
    ($marker:ident, $repo:ty, |$held:ident| $built:expr) => {
        impl RepoFor<domain::$marker> for Ledgered {
            type Repo = $repo;

            fn build(&self, (): &()) -> Self::Repo {
                let $held = &self.0;
                $built
            }
        }
    };
}

repo!(Sessions, Nobody, |_held| Nobody);
repo!(Memberships, Nobody, |_held| Nobody);
repo!(Projects, Nobody, |_held| Nobody);
repo!(ApiKeys, Inert, |_held| Inert::default());
repo!(Audit, Inert, |_held| Inert::default());
repo!(AlertRules, AlertLedger, |held| held.clone());
repo!(AlertStates, AlertLedger, |held| held.clone());
repo!(AlertEvents, AlertLedger, |held| held.clone());
repo!(AlertOutbox, AlertLedger, |held| held.clone());
repo!(AlertChannels, AlertLedger, |held| held.clone());
repo!(AlertDuty, AlertLedger, |held| held.clone());

struct Watching;

impl Stack for Watching {
    type Repos = Ledgered;
    type Clock = Moving;
    type Hasher = Inert;
    type Mailer = Couriered;
    type Photos = NoPhotos;
    type SpanStore = ClickhouseReader;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Oidc = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Couriered;
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn spec_l2_1_an_error_rate_rule_goes_pending_then_firing_then_back_to_ok() {
    let Some(client) = prepared("ferristrace_alert_spec_l2_1").await else {
        return;
    };
    let mut spans = Vec::new();
    let mut seed = 0u32;
    for second in (0..25 * 60).step_by(10) {
        seed += 1;
        spans.push(a_span(seed, "checkout", "POST /pay", second, 20, false));
        if (6 * 60..12 * 60).contains(&second) {
            seed += 1;
            spans.push(a_span(seed, "checkout", "POST /pay", second + 1, 20, true));
        }
    }
    stored(&client, Scope::of(org(1)), spans).await;
    let ledger = AlertLedger::default();
    AlertRules::create(
        &ledger,
        org(1),
        &RuleDraft {
            name: RuleName::try_from("Checkout errors").expect("a name"),
            project: None,
            enabled: true,
            definition: Definition::new(DefinitionParts {
                signal: traces(Some("checkout"), None, TraceMeasure::ErrorRate),
                condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                window_secs: 300,
                pending_secs: 120,
                severity: AlertSeverity::Critical,
                no_data: NoDataPolicy::Ok,
                channels: Vec::new(),
            })
            .expect("a definition"),
        },
        AccountId::new(Uuid::from_u128(9)),
    )
    .await
    .expect("kept")
    .expect("fresh");
    let clock = Moving::default();
    let app = Ferristrace::<Watching>::new(
        Ledgered(ledger.clone()),
        Adapters {
            clock: clock.clone(),
            hasher: Inert::default(),
            mailer: Couriered::default(),
            webhooks: Couriered::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: Some(ClickhouseReader::new(client)),
            issue_store: None,
            log_store: None,
            metric_store: None,
            oidc: None,
            provision: ProvisionPolicy::default(),
        },
    );

    let mut statuses = Vec::new();
    for minute in [6, 7, 8, 9, 10, 18] {
        clock.to(minute * 60);
        let tick = app.evaluate_alerts().await.expect("a tick");
        assert!(
            matches!(
                tick,
                Tick::Evaluated {
                    rules: 1,
                    failed: 0,
                    ..
                }
            ),
            "{tick:?}"
        );
        let status = ledger.states()[0].1.state.status;
        statuses.push((minute, status));
    }

    assert_eq!(
        statuses,
        vec![
            (6, AlertStatus::Ok),
            (7, AlertStatus::Pending),
            (8, AlertStatus::Pending),
            (9, AlertStatus::Firing),
            (10, AlertStatus::Firing),
            (18, AlertStatus::Ok),
        ]
    );
    let history: Vec<(AlertStatus, AlertStatus, u64)> = ledger
        .events()
        .iter()
        .map(|event| (event.from, event.to, event.at_ms / 1_000 - BASE_SECONDS))
        .collect();
    assert_eq!(
        history,
        vec![
            (AlertStatus::Ok, AlertStatus::Pending, 7 * 60),
            (AlertStatus::Pending, AlertStatus::Firing, 9 * 60),
            (AlertStatus::Firing, AlertStatus::Ok, 18 * 60),
        ],
        "one history event per change"
    );
    let firing = &ledger.events()[1];
    let rate = firing.value.expect("a value");
    assert!(rate > 5.0 && rate <= 50.0, "{rate}");
    assert_eq!(firing.labels, Labels::none());
}
