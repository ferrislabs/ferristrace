use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{
    BatchLimits, ClickhouseIssues, ClickhouseLogSink, ClickhouseMigrator, ClickhouseSink,
};
use ferristrace_domain::{
    DeployFeed, LogRecord, LogSink, ProjectId, ReleaseFilter, ReleaseReader, Scope, Severity, Span,
    SpanEvent, SpanId, SpanKind, SpanSink, SpanStatus, TenantId, TraceId, Window,
};
use ferristrace_infra::FixedClock;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const BASE: u64 = 1_700_000_000_000_000_000;
const SECOND: u64 = 1_000_000_000;
const MINUTE: u64 = 60 * SECOND;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

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

fn project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn everything() -> Window {
    Window::new(BASE - 10 * DAY, BASE + 10 * DAY).expect("a window")
}

fn all() -> ReleaseFilter {
    ReleaseFilter::over(everything())
}

struct Sent {
    seed: u32,
    service: &'static str,
    version: Option<&'static str>,
    environment: Option<(&'static str, &'static str)>,
    pod: Option<&'static str>,
    at: u64,
    failed: bool,
    kind: SpanKind,
    events: Vec<SpanEvent>,
}

impl Sent {
    fn new(seed: u32, service: &'static str, version: &'static str, at: u64) -> Self {
        Self {
            seed,
            service,
            version: Some(version),
            environment: Some(("deployment.environment", "prod")),
            pod: None,
            at,
            failed: false,
            kind: SpanKind::Server,
            events: Vec::new(),
        }
    }

    fn unversioned(seed: u32, service: &'static str, at: u64) -> Self {
        Self {
            version: None,
            ..Self::new(seed, service, "", at)
        }
    }

    fn failing(mut self) -> Self {
        self.failed = true;
        self
    }

    fn on_pod(mut self, pod: &'static str) -> Self {
        self.pod = Some(pod);
        self
    }

    fn in_environment(mut self, key: &'static str, value: &'static str) -> Self {
        self.environment = Some((key, value));
        self
    }

    fn without_environment(mut self) -> Self {
        self.environment = None;
        self
    }

    fn of_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    fn throwing(mut self, message: &str) -> Self {
        self.events.push(SpanEvent {
            time_unix_nanos: BASE + self.at,
            name: "exception".into(),
            attributes: BTreeMap::from([
                ("exception.type".into(), "TimeoutError".into()),
                ("exception.message".into(), message.into()),
            ]),
        });
        self
    }

    fn span(self) -> Span {
        let id = self.seed.to_be_bytes();
        let mut trace = [9u8; 16];
        trace[12..].copy_from_slice(&id);
        let mut span = [7u8; 8];
        span[4..].copy_from_slice(&id);
        let mut attributes = BTreeMap::new();
        if let Some(version) = self.version {
            attributes.insert("service.version".into(), version.into());
        }
        if let Some((key, value)) = self.environment {
            attributes.insert(key.into(), value.into());
        }
        if let Some(pod) = self.pod {
            attributes.insert("k8s.pod.name".into(), pod.into());
        }
        Span {
            trace_id: TraceId::try_from(trace.as_slice()).expect("sixteen bytes"),
            span_id: SpanId::try_from(span.as_slice()).expect("eight bytes"),
            parent: None,
            name: "pay".into(),
            service_name: self.service.into(),
            kind: self.kind,
            status: if self.failed {
                SpanStatus::Error
            } else {
                SpanStatus::Ok
            },
            start_unix_nanos: BASE + self.at,
            end_unix_nanos: BASE + self.at + SECOND,
            attributes,
            events: self.events,
        }
    }
}

async fn written(client: &Client, scope: Scope, sent: Vec<Sent>) {
    let sink = ClickhouseSink::new(
        client,
        BatchLimits {
            max_rows: 100_000,
            period: Duration::from_secs(60),
        },
        Duration::from_secs(7 * 24 * 60 * 60),
        FixedClock::at(NOW),
    );
    sink.accept(scope, sent.into_iter().map(Sent::span).collect())
        .await
        .expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
}

fn a_deploy_log(
    at: u64,
    service: &str,
    version: &str,
    environment: &str,
    event: &str,
) -> LogRecord {
    LogRecord {
        time_unix_nanos: BASE + at,
        observed_unix_nanos: BASE + at,
        severity: Severity::Info,
        severity_number: 9,
        service_name: service.into(),
        body: "deployed".into(),
        attributes: BTreeMap::from([("event.name".into(), event.into())]),
        resource: BTreeMap::from([
            ("service.version".into(), version.into()),
            ("deployment.environment.name".into(), environment.into()),
        ]),
        trace_id: None,
        span_id: None,
    }
}

async fn logged(client: &Client, scope: Scope, records: Vec<LogRecord>) {
    let sink = ClickhouseLogSink::new(
        client,
        BatchLimits {
            max_rows: 100_000,
            period: Duration::from_secs(60),
        },
        Duration::from_secs(7 * 24 * 60 * 60),
        FixedClock::at(NOW),
    );
    sink.accept_logs(scope, records)
        .await
        .expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
}

#[tokio::test]
async fn spec_l3_3_spans_of_a_new_version_from_many_pods_and_batches_make_one_release_and_one_deploy()
 {
    let Some(client) = prepared("ferristrace_releases_pods").await else {
        return;
    };
    let scope = Scope::of(org(1));
    written(
        &client,
        scope,
        vec![
            Sent::new(1, "checkout", "2.0.0", 10 * MINUTE).on_pod("checkout-a"),
            Sent::new(2, "checkout", "2.0.0", 10 * MINUTE + SECOND)
                .on_pod("checkout-b")
                .failing(),
        ],
    )
    .await;
    written(
        &client,
        scope,
        vec![
            Sent::new(3, "checkout", "2.0.0", 5 * MINUTE).on_pod("checkout-c"),
            Sent::new(4, "checkout", "2.0.0", 20 * MINUTE).on_pod("checkout-a"),
        ],
    )
    .await;
    written(
        &client,
        scope,
        vec![Sent::new(5, "checkout", "2.0.0", 30 * MINUTE).on_pod("checkout-b")],
    )
    .await;
    let reader = ClickhouseIssues::new(client.clone());

    let releases = reader.releases(org(1), &all()).await.expect("releases");
    let deploys = reader.deploys(org(1), &all()).await.expect("deploys");

    assert_eq!(releases.len(), 1);
    let release = &releases[0];
    assert_eq!(&*release.service, "checkout");
    assert_eq!(&*release.version, "2.0.0");
    assert_eq!(release.environments, vec![Box::<str>::from("prod")]);
    assert_eq!(release.first_seen_unix_nanos, BASE + 5 * MINUTE);
    assert_eq!(release.last_seen_unix_nanos, BASE + 30 * MINUTE);
    assert_eq!(release.requests, 5);
    assert_eq!(release.errors, 1);
    assert_eq!(deploys.len(), 1);
    assert_eq!(&*deploys[0].service, "checkout");
    assert_eq!(&*deploys[0].version, "2.0.0");
    assert_eq!(&*deploys[0].environment, "prod");
    assert_eq!(deploys[0].at_unix_nanos, BASE + 5 * MINUTE);

    client
        .query("OPTIMIZE TABLE releases FINAL")
        .execute()
        .await
        .expect("a merge");
    let merged = reader.releases(org(1), &all()).await.expect("releases");
    let rows: u64 = client
        .query("SELECT count() FROM releases")
        .fetch_one()
        .await
        .expect("a count");
    assert_eq!(merged, releases);
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn a_version_in_two_environments_is_one_release_and_two_deploys() {
    let Some(client) = prepared("ferristrace_releases_envs").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(1)),
        vec![
            Sent::new(1, "checkout", "2.0.0", 5 * MINUTE)
                .in_environment("deployment.environment", "staging"),
            Sent::new(2, "checkout", "2.0.0", 10 * MINUTE)
                .in_environment("deployment.environment.name", "prod"),
            Sent::new(3, "checkout", "2.0.0", 11 * MINUTE)
                .in_environment("deployment.environment.name", "prod"),
            Sent::new(4, "mailer", "9", 12 * MINUTE).without_environment(),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    let releases = reader.releases(org(1), &all()).await.expect("releases");
    let deploys = reader.deploys(org(1), &all()).await.expect("deploys");

    let checkout = releases
        .iter()
        .find(|release| &*release.service == "checkout")
        .expect("checkout");
    assert_eq!(
        checkout.environments,
        vec![Box::<str>::from("prod"), Box::<str>::from("staging")]
    );
    assert_eq!(checkout.requests, 3);
    let marks: Vec<(String, String)> = deploys
        .iter()
        .map(|deploy| (deploy.service.to_string(), deploy.environment.to_string()))
        .collect();
    assert_eq!(
        marks,
        vec![
            ("checkout".to_owned(), "staging".to_owned()),
            ("checkout".to_owned(), "prod".to_owned()),
            ("mailer".to_owned(), String::new()),
        ]
    );
    let mailer = releases
        .iter()
        .find(|release| &*release.service == "mailer")
        .expect("mailer");
    assert!(mailer.environments.is_empty());
}

#[tokio::test]
async fn spans_without_a_version_and_client_spans_make_no_request() {
    let Some(client) = prepared("ferristrace_releases_unversioned").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(1)),
        vec![
            Sent::unversioned(1, "checkout", 5 * MINUTE),
            Sent::new(2, "checkout", "3", 6 * MINUTE).of_kind(SpanKind::Client),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    let releases = reader.releases(org(1), &all()).await.expect("releases");

    assert_eq!(releases.len(), 1);
    assert_eq!(&*releases[0].version, "3");
    assert_eq!(releases[0].requests, 0);
    assert_eq!(releases[0].first_seen_unix_nanos, BASE + 6 * MINUTE);
}

#[tokio::test]
async fn spec_l3_3_a_deploy_log_record_makes_one_release_and_one_deploy_however_often_it_is_sent() {
    let Some(client) = prepared("ferristrace_releases_logs").await else {
        return;
    };
    let scope = Scope::of(org(1));
    logged(
        &client,
        scope,
        vec![
            a_deploy_log(2 * MINUTE, "billing", "5.1.0", "prod", "deploy"),
            a_deploy_log(3 * MINUTE, "billing", "5.1.0", "prod", "deployment.event"),
        ],
    )
    .await;
    logged(
        &client,
        scope,
        vec![
            a_deploy_log(4 * MINUTE, "billing", "5.1.0", "prod", "deploy"),
            a_deploy_log(5 * MINUTE, "billing", "5.2.0", "prod", "something.else"),
            a_deploy_log(6 * MINUTE, "billing", "", "prod", "deploy"),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    let releases = reader.releases(org(1), &all()).await.expect("releases");
    let deploys = reader.deploys(org(1), &all()).await.expect("deploys");

    assert_eq!(releases.len(), 1);
    assert_eq!(&*releases[0].service, "billing");
    assert_eq!(&*releases[0].version, "5.1.0");
    assert_eq!(releases[0].environments, vec![Box::<str>::from("prod")]);
    assert_eq!(releases[0].requests, 0);
    assert_eq!(releases[0].errors, 0);
    assert_eq!(deploys.len(), 1);
    assert_eq!(deploys[0].at_unix_nanos, BASE + 2 * MINUTE);
}

#[tokio::test]
async fn a_deploy_event_and_the_spans_of_the_same_version_are_one_deploy_at_the_earliest_instant() {
    let Some(client) = prepared("ferristrace_releases_event_then_spans").await else {
        return;
    };
    let scope = Scope::of(org(1));
    written(
        &client,
        scope,
        vec![Sent::new(1, "billing", "5.1.0", 10 * MINUTE)],
    )
    .await;
    logged(
        &client,
        scope,
        vec![a_deploy_log(
            3 * MINUTE,
            "billing",
            "5.1.0",
            "prod",
            "deploy",
        )],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    let deploys = reader.deploys(org(1), &all()).await.expect("deploys");
    let releases = reader.releases(org(1), &all()).await.expect("releases");

    assert_eq!(deploys.len(), 1);
    assert_eq!(deploys[0].at_unix_nanos, BASE + 3 * MINUTE);
    assert_eq!(releases[0].requests, 1);
}

#[tokio::test]
async fn deploys_are_windowed_by_when_they_happened_and_narrowed_by_service() {
    let Some(client) = prepared("ferristrace_releases_window").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(1)),
        vec![
            Sent::new(1, "checkout", "1", MINUTE),
            Sent::new(2, "checkout", "2", HOUR),
            Sent::new(3, "mailer", "7", HOUR),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);
    let inside = Window::new(BASE + 30 * MINUTE, BASE + 2 * HOUR).expect("a window");

    let windowed = reader
        .deploys(org(1), &ReleaseFilter::over(inside))
        .await
        .expect("deploys");
    let service = reader
        .deploys(
            org(1),
            &ReleaseFilter {
                service: Some("mailer".into()),
                ..ReleaseFilter::over(inside)
            },
        )
        .await
        .expect("deploys");
    let active = reader
        .releases(org(1), &ReleaseFilter::over(inside))
        .await
        .expect("releases");

    assert_eq!(windowed.len(), 2);
    assert!(windowed.iter().all(|deploy| &*deploy.version != "1"));
    assert_eq!(service.len(), 1);
    assert_eq!(&*service[0].service, "mailer");
    let versions: Vec<&str> = active.iter().map(|release| &*release.version).collect();
    assert!(!versions.contains(&"1"), "{versions:?}");
    assert!(versions.contains(&"2") && versions.contains(&"7"));
}

#[tokio::test]
async fn another_organisation_never_sees_a_release_a_deploy_or_a_detail() {
    let Some(client) = prepared("ferristrace_releases_tenants").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(1)),
        vec![Sent::new(1, "checkout", "2.0.0", 5 * MINUTE).throwing("secret failure")],
    )
    .await;
    logged(
        &client,
        Scope::of(org(1)),
        vec![a_deploy_log(MINUTE, "billing", "5", "prod", "deploy")],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    assert!(
        reader
            .releases(org(2), &all())
            .await
            .expect("releases")
            .is_empty()
    );
    assert!(
        reader
            .deploys(org(2), &all())
            .await
            .expect("deploys")
            .is_empty()
    );
    assert!(
        reader
            .release(org(2), "checkout", "2.0.0", None)
            .await
            .expect("a detail")
            .is_none()
    );
    assert!(
        reader
            .release(org(1), "checkout", "2.0.0", None)
            .await
            .expect("a detail")
            .is_some()
    );
}

#[tokio::test]
async fn a_project_bound_read_sees_only_the_releases_of_its_project() {
    let Some(client) = prepared("ferristrace_releases_projects").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(1)).in_project(Some(project(10))),
        vec![Sent::new(1, "checkout", "2.0.0", 5 * MINUTE)],
    )
    .await;
    written(
        &client,
        Scope::of(org(1)).in_project(Some(project(20))),
        vec![
            Sent::new(2, "checkout", "2.0.0", 6 * MINUTE),
            Sent::new(3, "mailer", "7", 7 * MINUTE),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);
    let narrowed = |seed| ReleaseFilter {
        project: Some(project(seed)),
        ..all()
    };

    let whole = reader.releases(org(1), &all()).await.expect("releases");
    let first = reader
        .releases(org(1), &narrowed(10))
        .await
        .expect("releases");
    let second = reader
        .deploys(org(1), &narrowed(20))
        .await
        .expect("deploys");
    let first_deploys = reader
        .deploys(org(1), &narrowed(10))
        .await
        .expect("deploys");
    let seen_by_first = reader
        .release(org(1), "mailer", "7", Some(project(10)))
        .await
        .expect("a detail");
    let seen_by_second = reader
        .release(org(1), "mailer", "7", Some(project(20)))
        .await
        .expect("a detail");

    assert_eq!(whole.len(), 2);
    assert_eq!(
        whole
            .iter()
            .find(|release| &*release.service == "checkout")
            .map(|release| release.requests),
        Some(2)
    );
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].requests, 1);
    assert_eq!(first_deploys.len(), 1);
    assert_eq!(second.len(), 2);
    assert!(seen_by_first.is_none());
    assert!(seen_by_second.is_some());
}

#[tokio::test]
async fn spec_l3_3_the_detail_compares_the_error_rate_in_the_hour_before_and_after() {
    let Some(client) = prepared("ferristrace_releases_rates").await else {
        return;
    };
    let deploy = 3 * HOUR;
    let mut sent = Vec::new();
    for seed in 0..10_u32 {
        let before = Sent::new(
            seed,
            "checkout",
            "1.0.0",
            deploy - 30 * MINUTE + u64::from(seed) * SECOND,
        );
        sent.push(if seed < 1 { before.failing() } else { before });
    }
    for seed in 10..14_u32 {
        let after = Sent::new(
            seed,
            "checkout",
            "2.0.0",
            deploy + 10 * MINUTE + u64::from(seed) * SECOND,
        );
        sent.push(if seed < 12 { after.failing() } else { after });
    }
    sent.push(Sent::new(20, "checkout", "1.0.0", deploy - 2 * HOUR).failing());
    sent.push(Sent::new(21, "checkout", "2.0.0", deploy + 2 * HOUR).failing());
    sent.push(Sent::new(22, "mailer", "1", deploy - MINUTE).failing());
    sent.push(
        Sent::new(23, "checkout", "1.0.0", deploy - MINUTE)
            .of_kind(SpanKind::Client)
            .failing(),
    );
    sent[10] = Sent::new(10, "checkout", "2.0.0", deploy).failing();
    written(&client, Scope::of(org(1)), sent).await;
    let reader = ClickhouseIssues::new(client);

    let detail = reader
        .release(org(1), "checkout", "2.0.0", None)
        .await
        .expect("a detail")
        .expect("a release");

    assert_eq!(detail.release.first_seen_unix_nanos, BASE + deploy);
    assert_eq!(detail.before.requests, 10);
    assert_eq!(detail.before.errors, 1);
    assert_eq!(detail.before.error_rate(), Some(0.1));
    assert_eq!(detail.after.requests, 4);
    assert_eq!(detail.after.errors, 2);
    assert_eq!(detail.after.error_rate(), Some(0.5));
}

#[tokio::test]
async fn spec_l3_3_the_detail_lists_the_issues_first_seen_in_the_release() {
    let Some(client) = prepared("ferristrace_releases_issues").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(1)),
        vec![
            Sent::new(1, "checkout", "1.0.0", 10 * MINUTE).throwing("card gateway offline"),
            Sent::new(2, "checkout", "2.0.0", 2 * HOUR).throwing("card gateway offline"),
            Sent::new(3, "checkout", "2.0.0", 2 * HOUR + MINUTE).throwing("ledger is locked"),
            Sent::new(4, "checkout", "2.0.0", 2 * HOUR + 2 * MINUTE).throwing("ledger is locked"),
            Sent::new(5, "checkout", "3.0.0", 5 * HOUR).throwing("cache poisoned"),
            Sent::new(6, "mailer", "1", 2 * HOUR + 3 * MINUTE).throwing("smtp refused"),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    let first = reader
        .release(org(1), "checkout", "1.0.0", None)
        .await
        .expect("a detail")
        .expect("a release");
    let second = reader
        .release(org(1), "checkout", "2.0.0", None)
        .await
        .expect("a detail")
        .expect("a release");
    let third = reader
        .release(org(1), "checkout", "3.0.0", None)
        .await
        .expect("a detail")
        .expect("a release");
    let listed = reader.releases(org(1), &all()).await.expect("releases");

    let messages = |detail: &ferristrace_domain::ReleaseDetail| {
        detail
            .introduced
            .iter()
            .map(|issue| issue.message.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(messages(&first), vec!["card gateway offline".to_owned()]);
    assert_eq!(messages(&second), vec!["ledger is locked".to_owned()]);
    assert_eq!(second.introduced[0].count, 2);
    assert_eq!(messages(&third), vec!["cache poisoned".to_owned()]);
    assert_eq!(second.release.new_issues, 1);
    let counts: Vec<(String, String, u64)> = listed
        .iter()
        .map(|release| {
            (
                release.service.to_string(),
                release.version.to_string(),
                release.new_issues,
            )
        })
        .collect();
    assert_eq!(
        counts,
        vec![
            ("checkout".to_owned(), "3.0.0".to_owned(), 1),
            ("mailer".to_owned(), "1".to_owned(), 1),
            ("checkout".to_owned(), "2.0.0".to_owned(), 1),
            ("checkout".to_owned(), "1.0.0".to_owned(), 1),
        ]
    );
}

#[tokio::test]
async fn the_feed_names_the_tenant_of_every_deploy_in_its_window() {
    let Some(client) = prepared("ferristrace_releases_feed").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(1)),
        vec![
            Sent::new(1, "checkout", "2.0.0", 5 * MINUTE),
            Sent::new(2, "checkout", "2.0.0", 6 * MINUTE),
        ],
    )
    .await;
    logged(
        &client,
        Scope::of(org(2)),
        vec![a_deploy_log(7 * MINUTE, "billing", "5", "prod", "deploy")],
    )
    .await;
    let reader = ClickhouseIssues::new(client);
    let window = Window::new(BASE, BASE + HOUR).expect("a window");
    let later = Window::new(BASE + 6 * MINUTE + SECOND, BASE + HOUR).expect("a window");

    let fed = reader.deployed(window).await.expect("a feed");
    let after = reader.deployed(later).await.expect("a feed");

    let seen: Vec<(TenantId, String)> = fed
        .iter()
        .map(|held| (held.org, held.deploy.service.to_string()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (org(1), "checkout".to_owned()),
            (org(2), "billing".to_owned())
        ]
    );
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].org, org(2));
}

#[tokio::test]
async fn the_migration_rolls_back_and_forward() {
    let Some(client) = prepared("ferristrace_releases_migrate").await else {
        return;
    };
    let migrator = ClickhouseMigrator::new(client.clone());

    let reverted = migrator.down_to(9).await.expect("rolled back");
    let tables: u64 = client
        .query("SELECT count() FROM system.tables WHERE database = currentDatabase() AND name LIKE 'releases%'")
        .fetch_one()
        .await
        .expect("a count");
    let forward = migrator.up().await.expect("rolled forward");

    assert_eq!(reverted, vec![10]);
    assert_eq!(tables, 0);
    assert_eq!(forward, vec![10]);
}

#[tokio::test]
async fn existing_spans_are_backfilled_when_the_migration_runs() {
    let Some(client) = prepared("ferristrace_releases_backfill").await else {
        return;
    };
    let migrator = ClickhouseMigrator::new(client.clone());
    migrator.down_to(9).await.expect("rolled back");
    written(
        &client,
        Scope::of(org(1)),
        vec![
            Sent::new(1, "checkout", "2.0.0", 5 * MINUTE),
            Sent::new(2, "checkout", "2.0.0", 9 * MINUTE),
        ],
    )
    .await;
    migrator.up().await.expect("rolled forward");
    let reader = ClickhouseIssues::new(client);

    let releases = reader.releases(org(1), &all()).await.expect("releases");

    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].requests, 2);
    assert_eq!(releases[0].first_seen_unix_nanos, BASE + 5 * MINUTE);
}
