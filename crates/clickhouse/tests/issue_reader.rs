use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseIssues, ClickhouseMigrator, ClickhouseSink};
use ferristrace_domain::{
    Fingerprint, IssueFilter, IssueReader, ProjectId, Scope, Span, SpanEvent, SpanId, SpanKind,
    SpanSink, SpanStatus, TenantId, TraceId, Window,
};
use ferristrace_infra::FixedClock;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const AN_HOUR: u64 = 3_600_000_000_000;
const A_SECOND: u64 = 1_000_000_000;
const T0: u64 = 1_700_000_000_000_000_000;

fn tenant(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn around_t0() -> Window {
    Window::new(T0 - AN_HOUR, T0 + AN_HOUR).expect("a window")
}

fn thrown(seed: u8, service: &str, events: Vec<SpanEvent>) -> Span {
    Span {
        trace_id: TraceId::try_from([seed; 16].as_slice()).expect("sixteen bytes"),
        span_id: SpanId::try_from([seed; 8].as_slice()).expect("eight bytes"),
        parent: None,
        name: "work".into(),
        service_name: service.into(),
        kind: SpanKind::Server,
        status: SpanStatus::Error,
        start_unix_nanos: T0,
        end_unix_nanos: T0 + A_SECOND,
        attributes: BTreeMap::new(),
        events,
    }
}

fn an_exception(at: u64, kind: &str, message: &str) -> SpanEvent {
    SpanEvent {
        time_unix_nanos: at,
        name: "exception".into(),
        attributes: BTreeMap::from([
            ("exception.type".into(), kind.into()),
            ("exception.message".into(), message.into()),
            (
                "exception.stacktrace".into(),
                "at charge (psp/client.ts:88)".into(),
            ),
        ]),
    }
}

fn a_log(at: u64) -> SpanEvent {
    SpanEvent {
        time_unix_nanos: at,
        name: "log".into(),
        attributes: BTreeMap::from([
            ("exception.type".into(), "TimeoutError".into()),
            ("exception.message".into(), "deadline exceeded".into()),
        ]),
    }
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

async fn stored(client: &Client, scope: Scope, spans: Vec<Span>) {
    let sink = ClickhouseSink::new(
        client,
        BatchLimits {
            max_rows: 1_000,
            period: Duration::from_secs(60 * 60),
        },
        Duration::from_secs(7 * 24 * 60 * 60),
        FixedClock::at(NOW),
    );
    sink.accept(scope, spans)
        .await
        .expect("the batch is accepted");
    sink.flush().await.expect("the sink flushes");
}

#[tokio::test]
async fn exceptions_that_differ_only_in_digits_are_one_issue_counted_twice() {
    let Some(client) = prepared("ferristrace_issues_grouped").await else {
        return;
    };
    let scope = Scope::of(tenant(1)).in_project(Some(project(10)));
    stored(
        &client,
        scope,
        vec![
            thrown(
                1,
                "checkout",
                vec![an_exception(
                    T0,
                    "TimeoutError",
                    "deadline exceeded after 960ms",
                )],
            ),
            thrown(
                2,
                "checkout",
                vec![an_exception(
                    T0 + A_SECOND,
                    "TimeoutError",
                    "deadline exceeded after 1200ms",
                )],
            ),
        ],
    )
    .await;

    let issues = ClickhouseIssues::new(client)
        .issues(tenant(1), &IssueFilter::over(around_t0()))
        .await
        .expect("the list is read");

    assert_eq!(
        issues.len(),
        1,
        "two messages that differ in digits are one issue"
    );
    let issue = &issues[0];
    assert_eq!(issue.project, project(10));
    assert_eq!(
        issue.print,
        Fingerprint::of("checkout", "TimeoutError", "deadline exceeded after 960ms"),
        "the stored print must be the one the domain mints"
    );
    assert_eq!(issue.count, 2);
    assert_eq!(&*issue.exception_type, "TimeoutError");
    assert_eq!(&*issue.service, "checkout");
    assert_eq!(
        &*issue.message, "deadline exceeded after 1200ms",
        "the message shown is the latest one"
    );
    assert_eq!(
        (issue.first_seen_unix_nanos, issue.last_seen_unix_nanos),
        (T0, T0 + A_SECOND)
    );
}

#[tokio::test]
async fn a_different_type_service_or_project_is_a_different_issue() {
    let Some(client) = prepared("ferristrace_issues_split").await else {
        return;
    };
    stored(
        &client,
        Scope::of(tenant(1)).in_project(Some(project(10))),
        vec![
            thrown(
                1,
                "checkout",
                vec![an_exception(T0, "TimeoutError", "deadline exceeded")],
            ),
            thrown(
                2,
                "checkout",
                vec![an_exception(T0, "DbError", "deadline exceeded")],
            ),
            thrown(
                3,
                "payments",
                vec![an_exception(T0, "TimeoutError", "deadline exceeded")],
            ),
        ],
    )
    .await;
    stored(
        &client,
        Scope::of(tenant(1)).in_project(Some(project(11))),
        vec![thrown(
            4,
            "checkout",
            vec![an_exception(T0, "TimeoutError", "deadline exceeded")],
        )],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    let all = reader
        .issues(tenant(1), &IssueFilter::over(around_t0()))
        .await
        .expect("the list is read");
    assert_eq!(
        all.len(),
        4,
        "type, service and project each split an issue"
    );
    assert!(all.iter().all(|issue| issue.count == 1));

    let mut filter = IssueFilter::over(around_t0());
    filter.project = Some(project(10));
    let one_project = reader
        .issues(tenant(1), &filter)
        .await
        .expect("the list is read");
    assert_eq!(one_project.len(), 3);
    assert!(one_project.iter().all(|issue| issue.project == project(10)));
}

#[tokio::test]
async fn an_event_that_is_not_an_exception_never_becomes_an_issue() {
    let Some(client) = prepared("ferristrace_issues_only_exceptions").await else {
        return;
    };
    stored(
        &client,
        Scope::of(tenant(1)).in_project(Some(project(10))),
        vec![
            thrown(1, "checkout", vec![a_log(T0), a_log(T0 + A_SECOND)]),
            thrown(
                2,
                "checkout",
                vec![
                    a_log(T0),
                    an_exception(T0 + A_SECOND, "TimeoutError", "deadline exceeded"),
                ],
            ),
        ],
    )
    .await;

    let issues = ClickhouseIssues::new(client)
        .issues(tenant(1), &IssueFilter::over(around_t0()))
        .await
        .expect("the list is read");

    assert_eq!(
        issues.len(),
        1,
        "log events carrying exception keys are not issues"
    );
    assert_eq!(
        issues[0].count, 1,
        "the log on the same span must not be counted"
    );
}

#[tokio::test]
async fn counting_is_scoped_to_the_window() {
    let Some(client) = prepared("ferristrace_issues_window").await else {
        return;
    };
    stored(
        &client,
        Scope::of(tenant(1)).in_project(Some(project(10))),
        vec![
            thrown(
                1,
                "checkout",
                vec![an_exception(T0 - 3 * AN_HOUR, "StaleError", "gone")],
            ),
            thrown(
                2,
                "checkout",
                vec![an_exception(
                    T0 - 3 * AN_HOUR,
                    "TimeoutError",
                    "deadline exceeded",
                )],
            ),
            thrown(
                3,
                "checkout",
                vec![an_exception(T0, "TimeoutError", "deadline exceeded")],
            ),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    let recent = reader
        .issues(tenant(1), &IssueFilter::over(around_t0()))
        .await
        .expect("the list is read");
    assert_eq!(
        recent.len(),
        1,
        "an issue with no occurrence in the window is absent"
    );
    assert_eq!(&*recent[0].exception_type, "TimeoutError");
    assert_eq!(
        recent[0].count, 1,
        "an occurrence outside the window is not counted"
    );
    assert_eq!(recent[0].first_seen_unix_nanos, T0);

    let wide = IssueFilter::over(Window::new(T0 - 4 * AN_HOUR, T0 + AN_HOUR).expect("a window"));
    let whole = reader
        .issues(tenant(1), &wide)
        .await
        .expect("the list is read");
    assert_eq!(whole.len(), 2);
    let timeouts = whole
        .iter()
        .find(|issue| &*issue.exception_type == "TimeoutError")
        .expect("the timeout issue");
    assert_eq!(timeouts.count, 2);
    assert_eq!(timeouts.first_seen_unix_nanos, T0 - 3 * AN_HOUR);
}

#[tokio::test]
async fn another_tenants_exceptions_are_never_counted() {
    let Some(client) = prepared("ferristrace_issues_tenants").await else {
        return;
    };
    stored(
        &client,
        Scope::of(tenant(1)).in_project(Some(project(10))),
        vec![thrown(
            1,
            "checkout",
            vec![an_exception(T0, "TimeoutError", "deadline exceeded")],
        )],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    let theirs = reader
        .issues(tenant(2), &IssueFilter::over(around_t0()))
        .await
        .expect("the list is read");
    assert!(
        theirs.is_empty(),
        "a tenant must not see another one's issues"
    );

    let print = Fingerprint::of("checkout", "TimeoutError", "deadline exceeded");
    let occurrences = reader
        .occurrences(tenant(2), project(10), print, around_t0(), 10)
        .await
        .expect("the occurrences are read");
    assert!(occurrences.is_empty(), "nor another one's occurrences");
}

#[tokio::test]
async fn occurrences_answer_trace_ids_newest_first() {
    let Some(client) = prepared("ferristrace_issues_occurrences").await else {
        return;
    };
    stored(
        &client,
        Scope::of(tenant(1)).in_project(Some(project(10))),
        vec![
            thrown(
                1,
                "checkout",
                vec![an_exception(
                    T0,
                    "TimeoutError",
                    "deadline exceeded after 1ms",
                )],
            ),
            thrown(
                2,
                "checkout",
                vec![an_exception(
                    T0 + A_SECOND,
                    "TimeoutError",
                    "deadline exceeded after 2ms",
                )],
            ),
            thrown(
                3,
                "checkout",
                vec![an_exception(
                    T0 + 2 * A_SECOND,
                    "TimeoutError",
                    "deadline exceeded after 3ms",
                )],
            ),
            thrown(
                4,
                "checkout",
                vec![an_exception(T0 + 3 * A_SECOND, "DbError", "refused")],
            ),
        ],
    )
    .await;
    let print = Fingerprint::of("checkout", "TimeoutError", "deadline exceeded after 1ms");

    let occurrences = ClickhouseIssues::new(client)
        .occurrences(tenant(1), project(10), print, around_t0(), 2)
        .await
        .expect("the occurrences are read");

    assert_eq!(occurrences.len(), 2, "the limit holds");
    let newest = &occurrences[0];
    assert_eq!(
        newest.trace_id,
        TraceId::try_from([3; 16].as_slice()).expect("sixteen bytes"),
        "the newest occurrence comes first"
    );
    assert_eq!(
        newest.span_id,
        SpanId::try_from([3; 8].as_slice()).expect("eight bytes")
    );
    assert_eq!(newest.time_unix_nanos, T0 + 2 * A_SECOND);
    assert_eq!(&*newest.service, "checkout");
    assert_eq!(&*newest.exception_type, "TimeoutError");
    assert_eq!(&*newest.message, "deadline exceeded after 3ms");
    assert_eq!(
        newest.stacktrace.as_deref(),
        Some("at charge (psp/client.ts:88)")
    );
    assert_eq!(
        occurrences[1].trace_id,
        TraceId::try_from([2; 16].as_slice()).expect("sixteen bytes")
    );
}

#[tokio::test]
async fn the_list_narrows_by_service_text_and_print() {
    let Some(client) = prepared("ferristrace_issues_filtered").await else {
        return;
    };
    stored(
        &client,
        Scope::of(tenant(1)).in_project(Some(project(10))),
        vec![
            thrown(
                1,
                "checkout",
                vec![an_exception(T0, "TimeoutError", "deadline exceeded")],
            ),
            thrown(
                2,
                "payments",
                vec![an_exception(T0, "TimeoutError", "deadline exceeded")],
            ),
            thrown(
                3,
                "checkout",
                vec![an_exception(T0, "DbError", "connection refused")],
            ),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);

    let mut by_service = IssueFilter::over(around_t0());
    by_service.service = Some("payments".into());
    let payments = reader
        .issues(tenant(1), &by_service)
        .await
        .expect("the list is read");
    assert_eq!(payments.len(), 1);
    assert_eq!(&*payments[0].service, "payments");

    let mut by_message = IssueFilter::over(around_t0());
    by_message.text = Some("REFUSED".into());
    let refused = reader
        .issues(tenant(1), &by_message)
        .await
        .expect("the list is read");
    assert_eq!(refused.len(), 1, "text matches the message, case aside");
    assert_eq!(&*refused[0].exception_type, "DbError");

    let mut by_type = IssueFilter::over(around_t0());
    by_type.text = Some("timeout".into());
    let timeouts = reader
        .issues(tenant(1), &by_type)
        .await
        .expect("the list is read");
    assert_eq!(timeouts.len(), 2, "text matches the type too");

    let mut by_print = IssueFilter::over(around_t0());
    by_print.print = Some(Fingerprint::of(
        "payments",
        "TimeoutError",
        "deadline exceeded",
    ));
    let one = reader
        .issues(tenant(1), &by_print)
        .await
        .expect("the list is read");
    assert_eq!(one.len(), 1);
    assert_eq!(&*one[0].service, "payments");

    let page = reader
        .issues(tenant(1), &IssueFilter::over(around_t0()).at_most(2))
        .await
        .expect("the list is read");
    assert_eq!(page.len(), 2, "the limit holds");
}
