use std::num::NonZeroU16;

use clickhouse::{Client, Row};
use ferristrace_clickhouse::{
    COLD_VOLUME, ClickhouseMigrator, ClickhouseTiering, MOVED_TABLES, SpanStoreSettings,
    TIERED_POLICY, open_span_store,
};
use ferristrace_infra::FixedClock;
use serde::Deserialize;

mod backend;

const NOW: u64 = 4_000_000_000;

#[derive(Debug, Row, Deserialize)]
struct Table {
    storage_policy: String,
    create_table_query: String,
}

#[derive(Debug, Row, Deserialize)]
struct Part {
    days_old: i64,
    disk_name: String,
}

#[derive(Debug, Row, Deserialize)]
struct Count {
    value: u64,
}

#[derive(Debug, Row, Deserialize)]
struct Disk {
    disk: String,
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

fn seven() -> NonZeroU16 {
    NonZeroU16::new(7).expect("seven is not zero")
}

async fn fresh_database(name: &str) -> Option<(String, Client)> {
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
    Some((url.clone(), connected(&url).with_database(name)))
}

async fn tiered_database(name: &str) -> Option<(String, Client)> {
    let (url, client) = fresh_database(name).await?;
    let cold: Vec<Disk> = client
        .query(
            "SELECT arrayJoin(disks) AS disk FROM system.storage_policies \
             WHERE policy_name = ? AND volume_name = ?",
        )
        .bind(TIERED_POLICY)
        .bind(COLD_VOLUME)
        .fetch_all()
        .await
        .expect("reading the storage policies");
    if cold.is_empty() {
        assert!(
            std::env::var("REQUIRE_CLICKHOUSE_URL").is_err(),
            "REQUIRE_CLICKHOUSE_URL is set but ClickHouse has no `tiered` policy \
             (see .github/clickhouse/tiered.xml)"
        );
        return None;
    }
    Some((url, client))
}

fn settings(url: &str, database: &str, cold_after_days: Option<NonZeroU16>) -> SpanStoreSettings {
    SpanStoreSettings {
        database: Some(database.to_owned()),
        user: std::env::var("CLICKHOUSE_USER").ok(),
        password: std::env::var("CLICKHOUSE_PASSWORD").ok(),
        cold_after_days,
        ..SpanStoreSettings::at(url)
    }
}

async fn table(client: &Client, name: &str) -> Table {
    client
        .query(
            "SELECT storage_policy, create_table_query FROM system.tables \
             WHERE database = currentDatabase() AND name = ?",
        )
        .bind(name)
        .fetch_one()
        .await
        .unwrap_or_else(|error| panic!("reading table {name}: {error}"))
}

async fn cold_disks(client: &Client) -> Vec<String> {
    client
        .query(
            "SELECT arrayJoin(disks) AS disk FROM system.storage_policies \
             WHERE policy_name = ? AND volume_name = ?",
        )
        .bind(TIERED_POLICY)
        .bind(COLD_VOLUME)
        .fetch_all::<Disk>()
        .await
        .expect("reading the cold disks")
        .into_iter()
        .map(|row| row.disk)
        .collect()
}

#[tokio::test]
async fn without_cold_days_the_moved_tables_keep_their_policy_and_only_delete() {
    let database = "ferristrace_cold_unset";
    let Some((url, client)) = fresh_database(database).await else {
        return;
    };

    open_span_store(&settings(&url, database, None), FixedClock::at(NOW))
        .await
        .expect("the span store opens without a cold volume");

    for moved in MOVED_TABLES {
        let shape = table(&client, moved.name).await;
        assert_eq!(shape.storage_policy, "default", "{}", moved.name);
        assert!(
            !shape.create_table_query.contains("TO VOLUME"),
            "{} gained a move: {}",
            moved.name,
            shape.create_table_query
        );
        assert!(
            shape.create_table_query.contains("TTL expire_at"),
            "{} lost its expiry: {}",
            moved.name,
            shape.create_table_query
        );
    }
}

#[tokio::test]
async fn seven_cold_days_move_the_large_tables_and_leave_the_catalogues() {
    let database = "ferristrace_cold_set";
    let Some((url, client)) = tiered_database(database).await else {
        return;
    };

    open_span_store(
        &settings(&url, database, Some(seven())),
        FixedClock::at(NOW),
    )
    .await
    .expect("the span store opens with a cold volume");

    for moved in MOVED_TABLES {
        let shape = table(&client, moved.name).await;
        assert_eq!(shape.storage_policy, TIERED_POLICY, "{}", moved.name);
        assert!(
            shape
                .create_table_query
                .contains("toIntervalDay(7) TO VOLUME 'cold', expire_at"),
            "{} has the wrong ttl: {}",
            moved.name,
            shape.create_table_query
        );
    }
    for catalogue in ["metric_catalogue", "releases", "infra_resources"] {
        assert_eq!(
            table(&client, catalogue).await.storage_policy,
            "default",
            "{catalogue}"
        );
    }
}

#[tokio::test]
async fn a_week_old_log_lands_on_the_cold_volume_and_is_still_read() {
    let database = "ferristrace_cold_moves";
    let Some((url, client)) = tiered_database(database).await else {
        return;
    };
    open_span_store(
        &settings(&url, database, Some(seven())),
        FixedClock::at(NOW),
    )
    .await
    .expect("the span store opens with a cold volume");

    client
        .query(
            "INSERT INTO logs \
             (tenant_id, project_id, service_name, time, observed_time, severity, \
              severity_number, body, expire_at) \
             SELECT generateUUIDv4(), generateUUIDv4(), 'checkout', t, t, 'info', 9, 'hello', \
                    now() + INTERVAL 20 DAY \
             FROM (SELECT arrayJoin([now64(9) - INTERVAL 8 DAY, now64(9) - INTERVAL 1 DAY]) AS t)",
        )
        .execute()
        .await
        .expect("writing an old and a recent log");
    client
        .query("ALTER TABLE logs MATERIALIZE TTL SETTINGS mutations_sync = 2")
        .execute()
        .await
        .expect("applying the ttl to the new parts");

    let cold = cold_disks(&client).await;
    let parts: Vec<Part> = client
        .query(
            "SELECT dateDiff('day', toDate(min_time), today()) AS days_old, disk_name \
             FROM system.parts \
             WHERE database = currentDatabase() AND table = 'logs' AND active \
             ORDER BY days_old",
        )
        .fetch_all()
        .await
        .expect("reading the log parts");

    assert_eq!(parts.len(), 2, "{parts:?}");
    assert!(
        parts[0].days_old <= 1 && parts[1].days_old >= 8,
        "{parts:?}"
    );
    assert!(!cold.contains(&parts[0].disk_name), "recent: {parts:?}");
    assert!(cold.contains(&parts[1].disk_name), "old: {parts:?}");
    let read = client
        .query("SELECT count() AS value FROM logs")
        .fetch_one::<Count>()
        .await
        .expect("counting the logs")
        .value;
    assert_eq!(read, 2);
}

#[tokio::test]
async fn opening_twice_leaves_the_same_policy_and_ttl() {
    let database = "ferristrace_cold_twice";
    let Some((url, client)) = tiered_database(database).await else {
        return;
    };
    let settings = settings(&url, database, Some(seven()));

    open_span_store(&settings, FixedClock::at(NOW))
        .await
        .expect("the first open");
    let first = table(&client, "spans").await;
    open_span_store(&settings, FixedClock::at(NOW))
        .await
        .expect("the second open");
    let second = table(&client, "spans").await;

    assert_eq!(first.storage_policy, second.storage_policy);
    assert_eq!(first.create_table_query, second.create_table_query);
}

#[tokio::test]
async fn unsetting_the_days_stops_the_moves_but_keeps_the_policy() {
    let database = "ferristrace_cold_unset_after";
    let Some((_, client)) = tiered_database(database).await else {
        return;
    };
    ClickhouseMigrator::new(client.clone())
        .up()
        .await
        .expect("the migrations apply");
    let tiering = ClickhouseTiering::new(client.clone());

    tiering.apply(Some(seven())).await.expect("tiering");
    tiering.apply(None).await.expect("untiering");

    let spans = table(&client, "spans").await;
    assert_eq!(spans.storage_policy, TIERED_POLICY);
    assert!(
        !spans.create_table_query.contains("TO VOLUME"),
        "{}",
        spans.create_table_query
    );
}
