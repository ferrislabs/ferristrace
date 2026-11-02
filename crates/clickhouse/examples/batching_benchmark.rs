use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseMigrator, ClickhouseSink};
use ferristrace_domain::{Scope, Span, SpanId, SpanKind, SpanSink, SpanStatus, TenantId, TraceId};
use ferristrace_infra::SystemClock;
use uuid::Uuid;

const DATABASE: &str = "comettrace_benchmark";
const TOTAL_SPANS: usize = 60_000;
const SPANS_PER_CALL: usize = 100;
const TENANTS: u128 = 20;
const THRESHOLDS: [u64; 4] = [100, 1_000, 10_000, 60_000];

#[tokio::main]
async fn main() {
    let url = std::env::var("CLICKHOUSE_URL")
        .expect("CLICKHOUSE_URL must point at a server for this benchmark");

    println!("| max_rows | spans | inserts | wall time | spans/s |");
    println!("|---------:|------:|--------:|----------:|--------:|");

    for threshold in THRESHOLDS {
        let client = fresh(&url).await;
        let sink = ClickhouseSink::new(
            &client,
            BatchLimits {
                max_rows: threshold,
                period: Duration::from_secs(60 * 60),
            },
            Duration::from_secs(7 * 24 * 60 * 60),
            SystemClock,
        );

        let started = Instant::now();
        for call in 0..(TOTAL_SPANS / SPANS_PER_CALL) {
            let tenant = TenantId::new(Uuid::from_u128((call as u128) % TENANTS));
            sink.accept(Scope::of(tenant), batch(call))
                .await
                .expect("accepted");
        }
        sink.flush().await.expect("the final flush");
        let elapsed = started.elapsed();

        let inserts = TOTAL_SPANS as u64 / threshold.max(1);
        let rate = TOTAL_SPANS as f64 / elapsed.as_secs_f64();
        println!(
            "| {threshold} | {TOTAL_SPANS} | {inserts} | {:.2}s | {rate:.0} |",
            elapsed.as_secs_f64()
        );
    }
}

fn batch(call: usize) -> Vec<Span> {
    (0..SPANS_PER_CALL)
        .map(|index| {
            let seed = u8::try_from((call + index) % 250).unwrap_or(1) + 1;
            Span {
                trace_id: TraceId::try_from([seed; 16].as_slice()).expect("sixteen bytes"),
                span_id: SpanId::try_from([seed; 8].as_slice()).expect("eight bytes"),
                parent: None,
                name: "GET /orders".into(),
                service_name: "checkout".into(),
                kind: SpanKind::Server,
                status: SpanStatus::Ok,
                start_unix_nanos: 1_700_000_000_000_000_000,
                end_unix_nanos: 1_700_000_000_500_000_000,
                attributes: BTreeMap::from([
                    ("http.status_code".into(), "200".into()),
                    ("http.route".into(), "/orders".into()),
                ]),
                events: Vec::new(),
            }
        })
        .collect()
}

async fn fresh(url: &str) -> Client {
    let mut root = Client::default().with_url(url);
    if let Ok(user) = std::env::var("CLICKHOUSE_USER") {
        root = root.with_user(user);
    }
    if let Ok(password) = std::env::var("CLICKHOUSE_PASSWORD") {
        root = root.with_password(password);
    }
    root.query(&format!("DROP DATABASE IF EXISTS {DATABASE}"))
        .execute()
        .await
        .expect("dropping the benchmark database");
    root.query(&format!("CREATE DATABASE {DATABASE}"))
        .execute()
        .await
        .expect("creating the benchmark database");
    let client = root.with_database(DATABASE);
    ClickhouseMigrator::new(client.clone())
        .up()
        .await
        .expect("the migrations apply");
    client
}
