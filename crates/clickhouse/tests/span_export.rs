use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::{
    ExportedSpan, ProjectId, Scope, Span, SpanCursor, SpanEvent, SpanId, SpanKind, SpanPages,
    SpanSink, SpanStatus, TenantId, TraceId,
};
use ferristrace_infra::FixedClock;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const T0: u64 = 1_700_000_000_000_000_000;
const A_SECOND: u64 = 1_000_000_000;
const AN_HOUR: u64 = 3_600 * A_SECOND;

fn tenant(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn span(seed: u16, service: &str, at: u64, events: Vec<SpanEvent>) -> Span {
    let [high, low] = seed.to_be_bytes();
    let mut trace = [0_u8; 16];
    trace[14] = high;
    trace[15] = low;
    let mut id = [0_u8; 8];
    id[6] = high;
    id[7] = low;
    Span {
        trace_id: TraceId::try_from(trace.as_slice()).expect("sixteen bytes"),
        span_id: SpanId::try_from(id.as_slice()).expect("eight bytes"),
        parent: seed
            .is_multiple_of(2)
            .then(|| SpanId::try_from([7_u8; 8].as_slice()).expect("eight")),
        name: format!("work-{seed}").into(),
        service_name: service.into(),
        kind: SpanKind::Server,
        status: SpanStatus::Ok,
        start_unix_nanos: at,
        end_unix_nanos: at + A_SECOND,
        attributes: BTreeMap::from([("seed".into(), seed.to_string().into())]),
        events,
    }
}

fn an_event(at: u64, name: &str) -> SpanEvent {
    SpanEvent {
        time_unix_nanos: at,
        name: name.into(),
        attributes: BTreeMap::from([("k".into(), "v".into())]),
    }
}

async fn stored(name: &str, rows: &[(TenantId, Option<ProjectId>, Vec<Span>)]) -> Option<Client> {
    let client = prepared(name).await?;
    let sink = ClickhouseSink::new(
        &client,
        BatchLimits {
            max_rows: 1_000,
            period: Duration::from_secs(60 * 60),
        },
        Duration::from_secs(60 * 60),
        FixedClock::at(NOW),
    );
    for (org, project, spans) in rows {
        let mut scope = Scope::of(*org);
        scope.project = *project;
        sink.accept(scope, spans.clone())
            .await
            .expect("the batch is accepted");
    }
    sink.flush().await.expect("the sink flushes");
    Some(client)
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
    ferristrace_clickhouse::ClickhouseMigrator::new(client.clone())
        .up()
        .await
        .expect("the migrations apply");
    Some(client)
}

async fn every_page(reader: &ClickhouseReader, org: TenantId, size: u32) -> Vec<Vec<ExportedSpan>> {
    let mut pages = Vec::new();
    let mut after: Option<SpanCursor> = None;
    loop {
        let page = reader
            .page(org, after.as_ref(), size)
            .await
            .expect("a page");
        pages.push(page.spans);
        match page.next {
            Some(next) => after = Some(next),
            None => return pages,
        }
    }
}

#[tokio::test]
async fn a_page_holds_only_the_spans_of_the_tenant_asked_for() {
    let ours = tenant(1);
    let theirs = tenant(2);
    let Some(client) = stored(
        "ferristrace_export_tenants",
        &[
            (
                ours,
                Some(project(10)),
                vec![span(1, "checkout", T0, Vec::new())],
            ),
            (
                theirs,
                Some(project(20)),
                vec![
                    span(2, "payments", T0, Vec::new()),
                    span(3, "checkout", T0, Vec::new()),
                ],
            ),
        ],
    )
    .await
    else {
        return;
    };
    let reader = ClickhouseReader::new(client);

    let ours_seen: Vec<_> = every_page(&reader, ours, 10)
        .await
        .into_iter()
        .flatten()
        .collect();
    let theirs_seen: Vec<_> = every_page(&reader, theirs, 10)
        .await
        .into_iter()
        .flatten()
        .collect();

    assert_eq!(ours_seen.len(), 1);
    assert_eq!(ours_seen[0].project, Some(project(10)));
    assert_eq!(ours_seen[0].span, span(1, "checkout", T0, Vec::new()));
    assert_eq!(theirs_seen.len(), 2);
    assert!(
        theirs_seen
            .iter()
            .all(|seen| seen.project == Some(project(20)))
    );
    assert!(
        every_page(&reader, tenant(3), 10)
            .await
            .into_iter()
            .flatten()
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn pages_cover_every_row_once_in_a_stable_order() {
    let ours = tenant(1);
    let mut spans = Vec::new();
    for seed in 1..=23_u16 {
        let service = ["alpha", "beta", "gamma"][usize::from(seed % 3)];
        spans.push(span(
            seed,
            service,
            T0 + u64::from(seed % 5) * AN_HOUR / 2 + u64::from(seed),
            Vec::new(),
        ));
    }
    let Some(client) = stored(
        "ferristrace_export_paging",
        &[
            (ours, None, spans.clone()),
            (tenant(2), None, vec![span(99, "alpha", T0, Vec::new())]),
        ],
    )
    .await
    else {
        return;
    };
    let reader = ClickhouseReader::new(client);

    let pages = every_page(&reader, ours, 5).await;

    assert_eq!(
        pages.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![5, 5, 5, 5, 3]
    );
    let flat: Vec<_> = pages.into_iter().flatten().collect();
    let mut seen: Vec<_> = flat.iter().map(|row| row.span.span_id).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 23, "every span exactly once");
    assert!(flat.iter().all(|row| row.project.is_none()));
    let key = |row: &ExportedSpan| {
        (
            row.span.service_name.clone(),
            row.span.start_unix_nanos / AN_HOUR,
            row.span.trace_id,
            row.span.span_id,
        )
    };
    let keys: Vec<_> = flat.iter().map(key).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "ordered by service, hour, trace, span");
    let one_page = every_page(&reader, ours, 100).await;
    assert_eq!(one_page.len(), 1);
    assert_eq!(one_page[0], flat);
}

#[tokio::test]
async fn a_page_carries_the_events_of_its_spans_and_no_others() {
    let ours = tenant(1);
    let Some(client) = stored(
        "ferristrace_export_events",
        &[
            (
                ours,
                None,
                vec![
                    span(
                        1,
                        "alpha",
                        T0,
                        vec![an_event(T0 + 10, "first"), an_event(T0 + 20, "second")],
                    ),
                    span(2, "alpha", T0, Vec::new()),
                    span(3, "alpha", T0, vec![an_event(T0 + 30, "third")]),
                ],
            ),
            (
                tenant(2),
                None,
                vec![span(1, "alpha", T0, vec![an_event(T0 + 40, "theirs")])],
            ),
        ],
    )
    .await
    else {
        return;
    };
    let reader = ClickhouseReader::new(client);

    let flat: Vec<_> = every_page(&reader, ours, 2)
        .await
        .into_iter()
        .flatten()
        .collect();

    let names: Vec<Vec<&str>> = flat
        .iter()
        .map(|row| row.span.events.iter().map(|e| &*e.name).collect())
        .collect();
    assert_eq!(names, vec![vec!["first", "second"], vec![], vec!["third"]]);
    assert!(
        flat.iter()
            .flat_map(|row| &row.span.events)
            .all(|event| &*event.name != "theirs")
    );
}
