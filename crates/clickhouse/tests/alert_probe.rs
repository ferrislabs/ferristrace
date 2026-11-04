use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use clickhouse::Client;
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{
    AlertLedger, Bearers, Couriered, Crew, IncidentLedger, Inert, OncallLedger, PriceLedger,
    StatusLedger, Untransacted,
};
use ferristrace_application::{Adapters, Ferristrace, SignalReaders, Stack};
use ferristrace_clickhouse::{
    BatchLimits, ClickhouseIssues, ClickhouseLogReader, ClickhouseLogSink, ClickhouseMetricReader,
    ClickhouseMetricSink, ClickhouseMigrator, ClickhouseReader, ClickhouseSink,
};
use ferristrace_domain::{
    AccountId, Aggregation, AlertRules, AlertSeverity, AlertStatus, Assigned, Author, Change,
    Clock, CommentBody, Comparison, Condition, Created, Definition, DefinitionParts, EntryKind,
    IncidentDraft, IncidentPatch, IncidentSeverity, IncidentStatus, Labels, LogRecord, LogSignal,
    LogSink, Membership, Memberships, Metric, MetricData, MetricPoint, MetricSignal, MetricSink,
    NoDataPolicy, NoPhotos, Observation, OrgRole, Project, ProjectId, ProjectName, Projects,
    Reader, RuleDraft, RuleName, Scope, Session, SessionError, SessionStore, SessionToken,
    Severity, Signal, SignalProbe, Span, SpanId, SpanKind, SpanSink, SpanStatus, StoreError,
    TeamId, TenantId, Tick, TraceId, TraceMeasure, TraceSignal, UnixSeconds, WhoamiError, Window,
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
        placement: ferristrace_domain::Placement::default(),
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
    let prices = PriceLedger::default();
    let probe = SignalReaders::<_, Inert, Inert, Inert, PriceLedger>::new(
        Some(&reader),
        None,
        None,
        None,
        &prices,
    );
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
    let prices = PriceLedger::default();
    let probe = SignalReaders::<Inert, _, Inert, Inert, PriceLedger>::new(
        None,
        Some(&reader),
        None,
        None,
        &prices,
    );
    let errors = Signal::Logs(LogSignal {
        services: vec!["checkout".into()],
        severities: vec![Severity::Error, Severity::Fatal],
        text: Some("payment".into()),
        attributes: vec![("region".into(), "eu".into())],
        placement: ferristrace_domain::Placement::default(),
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
    let prices = PriceLedger::default();
    let probe = SignalReaders::<Inert, Inert, _, Inert, PriceLedger>::new(
        None,
        None,
        Some(&reader),
        None,
        &prices,
    );
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

const SESSION: &str = "ft.person.on_call";
const ON_CALL: u128 = 9;

#[derive(Clone)]
struct OnCall;

impl SessionStore for OnCall {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() != SESSION {
            return Err(SessionError::Unknown);
        }
        Ok(Session {
            account: AccountId::new(Uuid::from_u128(ON_CALL)),
            email: "oncall@example.com".try_into().expect("an address"),
            name: None,
            photo: None,
            org: None,
            expires_at: UnixSeconds::new(0),
        })
    }
}

impl Memberships for OnCall {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: org(1),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

#[derive(Clone, Default)]
struct Ledgered {
    alerts: AlertLedger,
    incidents: IncidentLedger,
    crew: Crew,
    oncall: OncallLedger,
    prices: PriceLedger,
}

impl Untransacted for Ledgered {}

macro_rules! repo {
    ($marker:ident, $repo:ty, |$held:ident| $built:expr) => {
        impl RepoFor<domain::$marker> for Ledgered {
            type Repo = $repo;

            fn build(&self, (): &()) -> Self::Repo {
                let $held = self;
                $built
            }
        }
    };
}

repo!(Sessions, OnCall, |_held| OnCall);
repo!(Memberships, OnCall, |_held| OnCall);
repo!(Projects, Nobody, |_held| Nobody);
repo!(ApiKeys, Inert, |_held| Inert::default());
repo!(Audit, Inert, |_held| Inert::default());
repo!(Roster, Crew, |held| held.crew.clone());
repo!(AlertRules, AlertLedger, |held| held.alerts.clone());
repo!(AlertStates, AlertLedger, |held| held.alerts.clone());
repo!(AlertEvents, AlertLedger, |held| held.alerts.clone());
repo!(AlertOutbox, AlertLedger, |held| held.alerts.clone());
repo!(AlertChannels, AlertLedger, |held| held.alerts.clone());
repo!(AlertDuty, AlertLedger, |held| held.alerts.clone());
repo!(Incidents, IncidentLedger, |held| held.incidents.clone());
repo!(IncidentTimeline, IncidentLedger, |held| held
    .incidents
    .clone());
repo!(IncidentAttachments, IncidentLedger, |held| held
    .incidents
    .clone());
repo!(ModelPrices, PriceLedger, |held| held.prices.clone());
repo!(Schedules, OncallLedger, |held| held.oncall.clone());
repo!(Pages, OncallLedger, |held| held.oncall.clone());
repo!(StatusOutbox, StatusLedger, |held| {
    let _ = held;
    StatusLedger::default()
});

struct Watching;

impl Stack for Watching {
    type Repos = Ledgered;
    type Clock = Moving;
    type Mailer = Couriered;
    type Photos = NoPhotos;
    type SpanStore = ClickhouseReader;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Users = Bearers;
    type Agents = Inert;
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
                page: None,
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
        Ledgered {
            alerts: ledger.clone(),
            ..Ledgered::default()
        },
        Adapters {
            clock: clock.clone(),
            mailer: Couriered::default(),
            webhooks: Couriered::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: Some(ClickhouseReader::new(client)),
            issue_store: None,
            log_store: None,
            metric_store: None,
            users: Some(Bearers),
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

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn spec_l2_4_an_incident_declared_from_a_firing_alert_hears_of_its_recovery_in_order() {
    let Some(client) = prepared("ferristrace_alert_spec_l2_4").await else {
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
    let ledgered = Ledgered::default();
    ledgered.crew.seat(
        org(1),
        AccountId::new(Uuid::from_u128(ON_CALL)),
        "oncall@example.com",
    );
    let rule = AlertRules::create(
        &ledgered.alerts,
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
                severity: AlertSeverity::Warning,
                no_data: NoDataPolicy::Ok,
                channels: Vec::new(),
                page: None,
            })
            .expect("a definition"),
        },
        AccountId::new(Uuid::from_u128(ON_CALL)),
    )
    .await
    .expect("kept")
    .expect("fresh")
    .id;
    let clock = Moving::default();
    let app = Ferristrace::<Watching>::new(
        ledgered.clone(),
        Adapters {
            clock: clock.clone(),
            mailer: Couriered::default(),
            webhooks: Couriered::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: Some(ClickhouseReader::new(client)),
            issue_store: None,
            log_store: None,
            metric_store: None,
            users: Some(Bearers),
        },
    );
    let me = Reader::Session(SessionToken::for_subject(SESSION));
    for minute in [7, 9] {
        clock.to(minute * 60);
        app.evaluate_alerts().await.expect("a tick");
    }

    clock.to(9 * 60 + 10);
    let declared = app
        .open_incident(
            &me,
            org(1),
            &IncidentDraft {
                alert: Some(rule),
                ..IncidentDraft::default()
            },
        )
        .await
        .expect("declared from the firing alert");
    let id = declared.incident.id;
    clock.to(9 * 60 + 20);
    app.amend_incident(
        &me,
        org(1),
        id,
        IncidentPatch {
            status: Some(IncidentStatus::Identified),
            ..IncidentPatch::default()
        },
    )
    .await
    .expect("identified");
    clock.to(9 * 60 + 30);
    app.amend_incident(
        &me,
        org(1),
        id,
        IncidentPatch {
            severity: Some(IncidentSeverity::Sev1),
            ..IncidentPatch::default()
        },
    )
    .await
    .expect("escalated");
    clock.to(9 * 60 + 40);
    app.comment_on_incident(
        &me,
        org(1),
        id,
        &CommentBody::try_from("rolled back the payment service").expect("a body"),
        false,
    )
    .await
    .expect("commented");
    for minute in [10, 18] {
        clock.to(minute * 60);
        app.evaluate_alerts().await.expect("a tick");
    }
    let dossier = app.incident(&me, org(1), id).await.expect("a read");

    assert_eq!(declared.incident.opened_from_alert, Some(rule));
    assert_eq!(declared.incident.severity, IncidentSeverity::Sev3);
    assert_eq!(declared.incident.title.as_str(), "Checkout errors");
    let told: Vec<(EntryKind, Author, u64)> = dossier
        .chronicle
        .entries
        .iter()
        .map(|entry| {
            (
                entry.change.kind(),
                entry.author.clone(),
                entry.at_ms / 1_000 - BASE_SECONDS,
            )
        })
        .collect();
    let person = Author::Account(AccountId::new(Uuid::from_u128(ON_CALL)));
    assert_eq!(
        told,
        vec![
            (EntryKind::Opened, person.clone(), 9 * 60 + 10),
            (EntryKind::Status, person.clone(), 9 * 60 + 20),
            (EntryKind::Severity, person.clone(), 9 * 60 + 30),
            (EntryKind::Comment, person, 9 * 60 + 40),
            (EntryKind::Alert, Author::System, 18 * 60),
        ],
        "the recovery lands after the earlier entries, and staying firing adds nothing"
    );
    let Change::Alert {
        rule: told_rule,
        from,
        to,
        ..
    } = &dossier.chronicle.entries[4].change
    else {
        panic!("an alert entry");
    };
    assert_eq!(
        (*told_rule, *from, *to),
        (rule, AlertStatus::Firing, AlertStatus::Ok)
    );
}

fn placed_on(pod: &str) -> ferristrace_domain::Placement {
    ferristrace_domain::Placement {
        pod: Some(ferristrace_domain::PlacementName::try_from(pod).expect("a pod")),
        ..ferristrace_domain::Placement::default()
    }
}

#[tokio::test]
async fn the_probe_counts_only_the_spans_and_logs_of_the_placement_a_rule_watches() {
    let Some(client) = prepared("ferristrace_alert_probe_placement").await else {
        return;
    };
    let on = |mut span: Span, pod: &str| {
        span.attributes.insert("k8s.pod.name".into(), pod.into());
        span
    };
    stored(
        &client,
        Scope::of(org(1)),
        vec![
            on(a_span(1, "checkout", "pay", 1, 10, true), "checkout-1"),
            on(a_span(2, "checkout", "pay", 2, 10, false), "checkout-1"),
            on(a_span(3, "checkout", "pay", 3, 10, true), "checkout-2"),
        ],
    )
    .await;
    let log_sink = ClickhouseLogSink::new(
        &client,
        limits(),
        Duration::from_secs(7 * DAY),
        FixedClock::at(NOW),
    );
    let log = |second: u64, pod: &str| LogRecord {
        time_unix_nanos: nanos(second),
        observed_unix_nanos: nanos(second),
        severity: Severity::Error,
        severity_number: 17,
        service_name: "checkout".into(),
        body: "declined".into(),
        attributes: BTreeMap::new(),
        resource: BTreeMap::from([("k8s.pod.name".into(), pod.into())]),
        trace_id: None,
        span_id: None,
    };
    log_sink
        .accept_logs(
            Scope::of(org(1)),
            vec![
                log(1, "checkout-1"),
                log(2, "checkout-2"),
                log(3, "checkout-2"),
            ],
        )
        .await
        .expect("the sink accepts");
    log_sink.flush().await.expect("the sink flushes");
    let spans = ClickhouseReader::new(client.clone());
    let logs = ClickhouseLogReader::new(client);
    let prices = PriceLedger::default();
    let probe = SignalReaders::<_, _, Inert, Inert, PriceLedger>::new(
        Some(&spans),
        Some(&logs),
        None,
        None,
        &prices,
    );
    let traced = |pod: &str, operation: Option<&str>| {
        Signal::Traces(TraceSignal {
            service: Some("checkout".into()),
            operation: operation.map(Into::into),
            environment: None,
            measure: TraceMeasure::Count,
            placement: placed_on(pod),
        })
    };
    let logged = |pod: &str| {
        Signal::Logs(LogSignal {
            services: Vec::new(),
            severities: vec![Severity::Error],
            text: None,
            attributes: Vec::new(),
            placement: placed_on(pod),
        })
    };

    let one = only(&observed(&probe, org(1), &traced("checkout-1", None), window(0, 60)).await);
    let operation = only(
        &observed(
            &probe,
            org(1),
            &traced("checkout-2", Some("pay")),
            window(0, 60),
        )
        .await,
    );
    let lines = only(&observed(&probe, org(1), &logged("checkout-2"), window(0, 60)).await);
    let stranger = only(&observed(&probe, org(2), &logged("checkout-2"), window(0, 60)).await);

    assert_eq!(one, Some(2.0));
    assert_eq!(operation, Some(1.0));
    assert_eq!(lines, Some(2.0));
    assert_eq!(stranger, Some(0.0));
}

fn blamed_span(seed: u32, pod: u32, failed: bool) -> Span {
    let mut span = a_span(
        seed,
        "checkout",
        "POST /pay",
        u64::from(seed % 50),
        10,
        failed,
    );
    span.attributes
        .insert("k8s.pod.name".into(), format!("checkout-{pod}").into());
    span.attributes.insert(
        "k8s.node.name".into(),
        if pod.is_multiple_of(2) {
            "node-a"
        } else {
            "node-b"
        }
        .into(),
    );
    span.attributes
        .insert("service.version".into(), "1.4.2".into());
    span
}

async fn named<P: SignalProbe>(
    probe: &P,
    tenant: TenantId,
    signal: &Signal,
    over: Window,
) -> ferristrace_domain::Culprits {
    probe
        .culprits(tenant, None, signal, over, &Labels::none())
        .await
        .expect("the probe names the culprits")
}

fn pods_of(culprits: &ferristrace_domain::Culprits) -> Vec<(&str, Option<&str>, u64)> {
    culprits
        .pods()
        .iter()
        .map(|pod| (&*pod.name, pod.node.as_deref(), pod.count))
        .collect()
}

#[tokio::test]
async fn spec_l3_5_a_trace_probe_names_five_pods_counts_the_rest_and_keeps_to_its_organisation() {
    let Some(client) = prepared("ferristrace_alert_culprits_traces").await else {
        return;
    };
    let mut spans = Vec::new();
    let mut seed = 0u32;
    for pod in 1..=7u32 {
        for _ in 0..(8 - pod) {
            seed += 1;
            spans.push(blamed_span(seed, pod, true));
        }
    }
    seed += 1;
    spans.push(blamed_span(seed, 8, false));
    stored(&client, Scope::of(org(1)), spans).await;
    stored(
        &client,
        Scope::of(org(2)),
        (500..520)
            .map(|seed| {
                let mut span = blamed_span(seed, 99, true);
                span.attributes
                    .insert("service.version".into(), "9.9.9".into());
                span.attributes
                    .insert("k8s.node.name".into(), "node-z".into());
                span
            })
            .collect(),
    )
    .await;
    let reader = ClickhouseReader::new(client);
    let prices = PriceLedger::default();
    let probe = SignalReaders::<_, Inert, Inert, Inert, PriceLedger>::new(
        Some(&reader),
        None,
        None,
        None,
        &prices,
    );

    let culprits = named(
        &probe,
        org(1),
        &traces(Some("checkout"), None, TraceMeasure::ErrorRate),
        window(0, 60),
    )
    .await;
    let elsewhere = named(
        &probe,
        org(3),
        &traces(Some("checkout"), None, TraceMeasure::ErrorRate),
        window(0, 60),
    )
    .await;

    assert_eq!(
        pods_of(&culprits),
        vec![
            ("checkout-1", Some("node-b"), 7),
            ("checkout-2", Some("node-a"), 6),
            ("checkout-3", Some("node-b"), 5),
            ("checkout-4", Some("node-a"), 4),
            ("checkout-5", Some("node-b"), 3),
        ]
    );
    assert_eq!(
        culprits.others(),
        2,
        "pods 6 and 7 are counted, the healthy pod 8 is not a culprit"
    );
    let mut nodes: Vec<&str> = culprits.nodes().iter().map(|node| &**node).collect();
    nodes.sort_unstable();
    assert_eq!(nodes, ["node-a", "node-b"]);
    let versions: Vec<&str> = culprits.versions().iter().map(|v| &**v).collect();
    assert_eq!(versions, ["1.4.2"]);
    assert!(elsewhere.is_empty(), "{elsewhere:?}");

    let narrowed = named(
        &probe,
        org(1),
        &traces(Some("checkout"), Some("GET /cart"), TraceMeasure::ErrorRate),
        window(0, 60),
    )
    .await;
    assert!(narrowed.is_empty(), "{narrowed:?}");
}

#[tokio::test]
async fn spec_l3_5_a_log_probe_names_five_pods_counts_the_rest_and_keeps_to_its_organisation() {
    let Some(client) = prepared("ferristrace_alert_culprits_logs").await else {
        return;
    };
    let sink = ClickhouseLogSink::new(
        &client,
        limits(),
        Duration::from_secs(7 * DAY),
        FixedClock::at(NOW),
    );
    let log = |second: u64, pod: u32, node: &str, version: &str| LogRecord {
        time_unix_nanos: nanos(second),
        observed_unix_nanos: nanos(second),
        severity: Severity::Error,
        severity_number: 17,
        service_name: "checkout".into(),
        body: "declined".into(),
        attributes: BTreeMap::new(),
        resource: BTreeMap::from([
            ("k8s.pod.name".into(), format!("checkout-{pod}").into()),
            ("k8s.node.name".into(), node.into()),
            ("service.version".into(), version.into()),
        ]),
        trace_id: None,
        span_id: None,
    };
    let mut mine = Vec::new();
    for pod in 1..=7u32 {
        for line in 0..(8 - pod) {
            let node = if pod.is_multiple_of(2) {
                "node-a"
            } else {
                "node-b"
            };
            mine.push(log(u64::from(line) + 1, pod, node, "1.4.2"));
        }
    }
    sink.accept_logs(Scope::of(org(1)), mine)
        .await
        .expect("the sink accepts");
    sink.accept_logs(
        Scope::of(org(2)),
        (0..10).map(|n| log(n + 1, 99, "node-z", "9.9.9")).collect(),
    )
    .await
    .expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
    let reader = ClickhouseLogReader::new(client);
    let prices = PriceLedger::default();
    let probe = SignalReaders::<Inert, _, Inert, Inert, PriceLedger>::new(
        None,
        Some(&reader),
        None,
        None,
        &prices,
    );
    let errors = Signal::Logs(LogSignal {
        services: vec!["checkout".into()],
        severities: vec![Severity::Error],
        text: None,
        attributes: Vec::new(),
        placement: ferristrace_domain::Placement::default(),
    });

    let culprits = named(&probe, org(1), &errors, window(0, 60)).await;
    let elsewhere = named(&probe, org(3), &errors, window(0, 60)).await;

    assert_eq!(
        pods_of(&culprits),
        vec![
            ("checkout-1", Some("node-b"), 7),
            ("checkout-2", Some("node-a"), 6),
            ("checkout-3", Some("node-b"), 5),
            ("checkout-4", Some("node-a"), 4),
            ("checkout-5", Some("node-b"), 3),
        ]
    );
    assert_eq!(culprits.others(), 2);
    let mut nodes: Vec<&str> = culprits.nodes().iter().map(|node| &**node).collect();
    nodes.sort_unstable();
    assert_eq!(nodes, ["node-a", "node-b"]);
    let versions: Vec<&str> = culprits.versions().iter().map(|v| &**v).collect();
    assert_eq!(versions, ["1.4.2"]);
    assert!(elsewhere.is_empty(), "{elsewhere:?}");
}

#[tokio::test]
async fn spec_l3_5_a_metric_probe_names_the_labels_of_the_series_that_fired() {
    let prices = PriceLedger::default();
    let probe = SignalReaders::<Inert, Inert, Inert, Inert, PriceLedger>::new(
        None, None, None, None, &prices,
    );
    let signal = Signal::Metrics(MetricSignal {
        name: "queue.depth".into(),
        aggregation: Aggregation::Max,
        group_by: vec!["queue".into()],
        filter: BTreeMap::new(),
    });
    let labels = Labels::new(BTreeMap::from([("queue".into(), "mail".into())]));

    let culprits = probe
        .culprits(org(1), None, &signal, window(0, 60), &labels)
        .await
        .expect("the labels need no read");

    assert_eq!(culprits.labels(), &labels);
    assert!(culprits.pods().is_empty());
}

#[tokio::test]
async fn spec_l3_5_a_firing_rule_stores_the_culprits_on_its_event() {
    let Some(client) = prepared("ferristrace_alert_culprits_event").await else {
        return;
    };
    let spans: Vec<Span> = (1..=9u32)
        .map(|seed| blamed_span(seed, seed, true))
        .collect();
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
                window_secs: 3_600,
                pending_secs: 0,
                severity: AlertSeverity::Critical,
                no_data: NoDataPolicy::Ok,
                channels: Vec::new(),
                page: None,
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
        Ledgered {
            alerts: ledger.clone(),
            ..Ledgered::default()
        },
        Adapters {
            clock: clock.clone(),
            mailer: Couriered::default(),
            webhooks: Couriered::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: Some(ClickhouseReader::new(client)),
            issue_store: None,
            log_store: None,
            metric_store: None,
            users: Some(Bearers),
        },
    );

    clock.to(60);
    app.evaluate_alerts().await.expect("a tick");

    let events = ledger.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].to, AlertStatus::Firing);
    let context = events[0].context.as_ref().expect("a context");
    assert_eq!(context.pods().len(), 5);
    assert_eq!(context.others(), 4);
    assert_eq!(context.nodes().len(), 2);
}

struct Spending;

impl Stack for Spending {
    type Repos = Ledgered;
    type Clock = Moving;
    type Mailer = Couriered;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = ClickhouseIssues;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Users = Bearers;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Couriered;
}

fn an_llm_call(seed: u32, second: u64, feature: &str, model: &str) -> Span {
    let mut span = a_span(seed, "assistant", "chat", second, 250, false);
    span.kind = SpanKind::Client;
    for (key, value) in [
        ("gen_ai.provider.name", "openai"),
        ("gen_ai.operation.name", "chat"),
        ("gen_ai.request.model", model),
        ("gen_ai.usage.input_tokens", "1000000"),
        ("gen_ai.usage.output_tokens", "0"),
        ("ferristrace.feature", feature),
    ] {
        span.attributes.insert(key.into(), value.into());
    }
    span
}

async fn gpt_4o_at_two_dollars_a_million() -> PriceLedger {
    let prices = PriceLedger::default();
    ferristrace_domain::ModelPrices::create(
        &prices,
        org(1),
        &ferristrace_domain::PriceDraft {
            provider: ferristrace_domain::Provider::try_from("openai").expect("a provider"),
            model: ferristrace_domain::ModelPattern::try_from("gpt-4o").expect("a model"),
            rates: ferristrace_domain::Rates::new(2_000_000, 0, None).expect("rates"),
            effective_from_ms: 0,
        },
        AccountId::new(Uuid::from_u128(9)),
    )
    .await
    .expect("kept")
    .expect("fresh");
    prices
}

fn spend_signal(
    measure: ferristrace_domain::LlmMeasure,
    group_by: ferristrace_domain::LlmGroup,
    feature: Option<&str>,
) -> Signal {
    Signal::Llm(ferristrace_domain::LlmSignal {
        measure,
        group_by,
        feature: feature.map(Into::into),
        customer: None,
        model: None,
        project: None,
    })
}

#[tokio::test]
async fn the_probe_reads_spend_for_one_group_or_one_per_feature() {
    let Some(client) = prepared("ferristrace_alert_probe_llm").await else {
        return;
    };
    let mut spans: Vec<Span> = (1..=3)
        .map(|seed| an_llm_call(seed, 10 * u64::from(seed), "ticket-summary", "gpt-4o"))
        .collect();
    spans.push(an_llm_call(10, 15, "search", "gpt-4o"));
    spans.push(an_llm_call(11, 16, "search", "mystery"));
    stored(&client, Scope::of(org(1)), spans).await;
    stored(
        &client,
        Scope::of(org(2)),
        vec![an_llm_call(20, 12, "ticket-summary", "gpt-4o")],
    )
    .await;
    let issues = ClickhouseIssues::new(client);
    let prices = gpt_4o_at_two_dollars_a_million().await;
    let probe = SignalReaders::<Inert, Inert, Inert, _, PriceLedger>::new(
        None,
        None,
        None,
        Some(&issues),
        &prices,
    );
    let minute = window(0, 60);
    let measure = ferristrace_domain::LlmMeasure::Cost;
    let group = ferristrace_domain::LlmGroup::None;

    let ticket = only(
        &observed(
            &probe,
            org(1),
            &spend_signal(measure, group, Some("ticket-summary")),
            minute,
        )
        .await,
    );
    let everything =
        only(&observed(&probe, org(1), &spend_signal(measure, group, None), minute).await);
    let calls = only(
        &observed(
            &probe,
            org(1),
            &spend_signal(ferristrace_domain::LlmMeasure::Calls, group, None),
            minute,
        )
        .await,
    );
    let per_feature = observed(
        &probe,
        org(1),
        &spend_signal(measure, ferristrace_domain::LlmGroup::Feature, None),
        minute,
    )
    .await;
    let nothing = observed(
        &probe,
        org(1),
        &spend_signal(measure, group, Some("absent")),
        minute,
    )
    .await;

    assert_eq!(ticket, Some(6.0));
    assert_eq!(everything, Some(8.0), "the unpriced call adds nothing");
    assert_eq!(calls, Some(5.0));
    let mut by_feature: Vec<(String, Option<f64>)> = per_feature
        .iter()
        .map(|seen| (seen.labels.pairs()["feature"].to_string(), seen.value))
        .collect();
    by_feature.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(
        by_feature,
        vec![
            ("search".to_owned(), Some(2.0)),
            ("ticket-summary".to_owned(), Some(6.0)),
        ]
    );
    assert!(nothing.is_empty());
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn spec_l4_4_a_cost_rule_fires_when_the_priced_calls_cross_ten_dollars_and_records_the_event()
{
    let Some(client) = prepared("ferristrace_alert_spec_l4_4").await else {
        return;
    };
    let mut spans: Vec<Span> = (1..=3)
        .map(|seed| an_llm_call(seed, 10 * u64::from(seed), "ticket-summary", "gpt-4o"))
        .collect();
    spans.extend(
        (4..=6)
            .map(|seed| an_llm_call(seed, 20 * 60 + u64::from(seed), "ticket-summary", "gpt-4o")),
    );
    spans.extend((7..=9).map(|seed| an_llm_call(seed, 30 + u64::from(seed), "search", "gpt-4o")));
    stored(&client, Scope::of(org(1)), spans).await;
    let ledger = AlertLedger::default();
    AlertRules::create(
        &ledger,
        org(1),
        &RuleDraft {
            name: RuleName::try_from("Ticket spend").expect("a name"),
            project: None,
            enabled: true,
            definition: Definition::new(DefinitionParts {
                signal: spend_signal(
                    ferristrace_domain::LlmMeasure::Cost,
                    ferristrace_domain::LlmGroup::Feature,
                    Some("ticket-summary"),
                ),
                condition: Condition::new(Comparison::Above, 10.0).expect("a threshold"),
                window_secs: 3_600,
                pending_secs: 0,
                severity: AlertSeverity::Critical,
                no_data: NoDataPolicy::Ok,
                channels: Vec::new(),
                page: None,
            })
            .expect("a definition"),
        },
        AccountId::new(Uuid::from_u128(9)),
    )
    .await
    .expect("kept")
    .expect("fresh");
    let clock = Moving::default();
    let app = Ferristrace::<Spending>::new(
        Ledgered {
            alerts: ledger.clone(),
            prices: gpt_4o_at_two_dollars_a_million().await,
            ..Ledgered::default()
        },
        Adapters {
            clock: clock.clone(),
            mailer: Couriered::default(),
            webhooks: Couriered::default(),
            photos: NoPhotos,
            meter: Inert::default(),
            span_store: None,
            issue_store: Some(ClickhouseIssues::new(client)),
            log_store: None,
            metric_store: None,
            users: Some(Bearers),
        },
    );

    clock.to(10 * 60);
    let early = app.evaluate_alerts().await.expect("a tick");
    let before = ledger.states()[0].1.state.status;
    clock.to(30 * 60);
    let late = app.evaluate_alerts().await.expect("a tick");

    assert!(
        matches!(
            early,
            Tick::Evaluated {
                rules: 1,
                failed: 0,
                ..
            }
        ),
        "{early:?}"
    );
    assert_eq!(before, AlertStatus::Ok);
    assert!(
        matches!(
            late,
            Tick::Evaluated {
                rules: 1,
                failed: 0,
                ..
            }
        ),
        "{late:?}"
    );
    let events = ledger.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].to, AlertStatus::Firing);
    assert_eq!(events[0].value, Some(12.0));
    let named = Labels::new(BTreeMap::from([(
        "feature".into(),
        "ticket-summary".into(),
    )]));
    assert_eq!(events[0].labels, named);
    assert_eq!(
        events[0].context.as_ref().expect("a context").labels(),
        &named
    );
}
