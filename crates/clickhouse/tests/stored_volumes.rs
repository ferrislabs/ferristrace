use clickhouse::Client;
use ferristrace_clickhouse::{ClickhouseMigrator, ClickhouseVolumes};
use ferristrace_domain::UnixSeconds;
use ferristrace_domain::telemetry::{BillingHour, HourSplit, StoredVolumes};
use uuid::Uuid;

mod backend;

const FIRST_HOUR: u64 = 1_700_000_400;
const NEXT_HOUR: u64 = FIRST_HOUR + 3600;
const MINE: &str = "00000000-0000-0000-0000-000000000001";
const OTHER: &str = "00000000-0000-0000-0000-000000000002";

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

async fn run(client: &Client, sql: String) {
    client.query(&sql).execute().await.expect("the insert runs");
}

async fn a_log(client: &Client, tenant: &str, at: u64) {
    run(
        client,
        format!(
            "INSERT INTO logs (tenant_id, project_id, service_name, time, observed_time, severity, \
             severity_number, body, attributes, resource, trace_id, span_id, expire_at) VALUES \
             ('{tenant}', '{tenant}', 'api', toDateTime64({at}, 9, 'UTC'), toDateTime64({at}, 9, 'UTC'), \
             'info', 9, 'hello', map(), map(), unhex('00000000000000000000000000000000'), \
             unhex('0000000000000000'), now() + INTERVAL 1 DAY)"
        ),
    )
    .await;
}

async fn a_point(client: &Client, tenant: &str, at: u64) {
    run(
        client,
        format!(
            "INSERT INTO metric_points (tenant_id, project_id, service_name, name, kind, temporality, \
             monotonic, attributes, time, value, expire_at) VALUES \
             ('{tenant}', '{tenant}', 'api', 'queue.depth', 'gauge', 'unspecified', 0, map(), \
             toDateTime64({at}, 9, 'UTC'), 1.5, now() + INTERVAL 1 DAY)"
        ),
    )
    .await;
}

fn hour(at: u64) -> BillingHour {
    BillingHour::containing(UnixSeconds::new(at))
}

fn found(splits: &[HourSplit], tenant: &str, at: u64) -> Option<HourSplit> {
    let tenant = Uuid::parse_str(tenant).expect("a uuid");
    splits
        .iter()
        .find(|split| split.tenant.as_uuid() == tenant && split.hour == hour(at))
        .copied()
}

#[tokio::test]
async fn each_signal_is_totalled_per_tenant_and_per_hour() {
    let Some(client) = prepared("ferristrace_stored_volumes").await else {
        return;
    };
    a_log(&client, MINE, FIRST_HOUR).await;
    a_log(&client, MINE, FIRST_HOUR + 60).await;
    a_log(&client, MINE, NEXT_HOUR).await;
    a_point(&client, MINE, FIRST_HOUR).await;
    a_point(&client, MINE, FIRST_HOUR + 1).await;
    a_point(&client, MINE, FIRST_HOUR + 2).await;
    a_log(&client, OTHER, FIRST_HOUR).await;

    let splits = ClickhouseVolumes::new(client)
        .hourly()
        .await
        .expect("the volumes are read");

    let first = found(&splits, MINE, FIRST_HOUR).expect("the first hour");
    assert_eq!(first.split.metrics_points, 3);
    assert!(first.split.logs_bytes > 0);
    assert_eq!(first.split.spans_bytes, 0);
    let next = found(&splits, MINE, NEXT_HOUR).expect("the next hour");
    assert_eq!(next.split.metrics_points, 0);
    assert!(next.split.logs_bytes > 0);
    assert!(
        first.split.logs_bytes > next.split.logs_bytes,
        "two records weigh more than one"
    );
    let other = found(&splits, OTHER, FIRST_HOUR).expect("the other tenant");
    assert_eq!(other.split.metrics_points, 0);
    assert_eq!(splits.len(), 3);
}
