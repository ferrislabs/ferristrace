use clickhouse::{Client, Row};
use ferristrace_clickhouse::{ClickhouseMigrator, MIGRATIONS};
use serde::Deserialize;

mod backend;

#[derive(Debug, Row, Deserialize)]
struct Text {
    value: String,
}

#[derive(Debug, Row, Deserialize)]
struct Count {
    value: u64,
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

async fn fresh_database(name: &str) -> Option<Client> {
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
    Some(connected(&url).with_database(name))
}

async fn one_text(client: &Client, query: &str) -> String {
    client
        .query(query)
        .fetch_one::<Text>()
        .await
        .expect("a single text value")
        .value
}

#[tokio::test]
async fn migrations_apply_once_and_rerunning_changes_nothing() {
    let Some(client) = fresh_database("ferristrace_migrate_once").await else {
        return;
    };
    let migrator = ClickhouseMigrator::new(client);

    let first = migrator.up().await.expect("the first run applies");
    let second = migrator.up().await.expect("the second run is a no-op");

    assert_eq!(
        first,
        MIGRATIONS.iter().map(|m| m.version).collect::<Vec<_>>()
    );
    assert!(second.is_empty(), "the second run applied {second:?}");
    assert_eq!(
        migrator.applied().await.expect("the recorded versions"),
        first
    );
}

#[tokio::test]
async fn the_spans_table_carries_the_intended_key_partition_and_ttl() {
    let Some(client) = fresh_database("ferristrace_migrate_shape").await else {
        return;
    };
    ClickhouseMigrator::new(client.clone())
        .up()
        .await
        .expect("the migrations apply");

    let sorting_key = one_text(
        &client,
        "SELECT sorting_key AS value FROM system.tables \
         WHERE database = currentDatabase() AND name = 'spans'",
    )
    .await;
    let partition_key = one_text(
        &client,
        "SELECT partition_key AS value FROM system.tables \
         WHERE database = currentDatabase() AND name = 'spans'",
    )
    .await;

    assert!(
        sorting_key.starts_with("tenant_id"),
        "the key must lead with the tenant, got {sorting_key}"
    );
    assert!(sorting_key.contains("service_name"), "{sorting_key}");
    assert!(
        sorting_key.contains("toStartOfHour(start_time)"),
        "{sorting_key}"
    );
    assert!(sorting_key.contains("trace_id"), "{sorting_key}");
    assert_eq!(partition_key, "toDate(start_time)");

    let ttl = one_text(
        &client,
        "SELECT create_table_query AS value FROM system.tables \
         WHERE database = currentDatabase() AND name = 'spans'",
    )
    .await;
    assert!(ttl.contains("TTL expire_at"), "{ttl}");
}

#[tokio::test]
async fn the_promoted_columns_sit_beside_the_attribute_map() {
    let Some(client) = fresh_database("ferristrace_migrate_columns").await else {
        return;
    };
    ClickhouseMigrator::new(client.clone())
        .up()
        .await
        .expect("the migrations apply");

    let attributes = one_text(
        &client,
        "SELECT type AS value FROM system.columns \
         WHERE database = currentDatabase() AND table = 'spans' AND name = 'attributes'",
    )
    .await;
    let http_status = one_text(
        &client,
        "SELECT type AS value FROM system.columns \
         WHERE database = currentDatabase() AND table = 'spans' AND name = 'http_status_code'",
    )
    .await;
    let service = one_text(
        &client,
        "SELECT type AS value FROM system.columns \
         WHERE database = currentDatabase() AND table = 'spans' AND name = 'service_name'",
    )
    .await;

    assert_eq!(attributes, "Map(String, String)");
    assert_eq!(http_status, "Nullable(UInt16)");
    assert_eq!(service, "LowCardinality(String)");
}

#[tokio::test]
async fn up_then_down_then_up_leaves_one_recorded_version() {
    let Some(client) = fresh_database("ferristrace_migrate_cycle").await else {
        return;
    };
    let migrator = ClickhouseMigrator::new(client.clone());

    migrator.up().await.expect("the first up");
    let reverted = migrator.down_to(0).await.expect("the down");
    let gone = client
        .query(
            "SELECT count() AS value FROM system.tables \
             WHERE database = currentDatabase() AND name = 'spans'",
        )
        .fetch_one::<Count>()
        .await
        .expect("counting the table")
        .value;
    let logs_gone = client
        .query(
            "SELECT count() AS value FROM system.tables \
             WHERE database = currentDatabase() AND name = 'logs'",
        )
        .fetch_one::<Count>()
        .await
        .expect("counting the table")
        .value;
    let metrics_gone = client
        .query(
            "SELECT count() AS value FROM system.tables \
             WHERE database = currentDatabase() AND name LIKE 'metric_%'",
        )
        .fetch_one::<Count>()
        .await
        .expect("counting the tables")
        .value;
    let again = migrator.up().await.expect("the second up");

    assert_eq!(reverted.len(), MIGRATIONS.len());
    assert_eq!(gone, 0, "the down step left the spans table behind");
    assert_eq!(logs_gone, 0, "the down step left the logs table behind");
    assert_eq!(metrics_gone, 0, "the down step left a metric table behind");
    assert_eq!(
        again,
        MIGRATIONS.iter().map(|m| m.version).collect::<Vec<_>>()
    );
    assert_eq!(
        migrator
            .applied()
            .await
            .expect("the recorded versions")
            .len(),
        MIGRATIONS.len()
    );
}

#[tokio::test]
async fn the_logs_table_carries_the_intended_key_partition_ttl_and_indexes() {
    let Some(client) = fresh_database("ferristrace_migrate_logs").await else {
        return;
    };
    ClickhouseMigrator::new(client.clone())
        .up()
        .await
        .expect("the migrations apply");

    let sorting_key = one_text(
        &client,
        "SELECT sorting_key AS value FROM system.tables \
         WHERE database = currentDatabase() AND name = 'logs'",
    )
    .await;
    let partition_key = one_text(
        &client,
        "SELECT partition_key AS value FROM system.tables \
         WHERE database = currentDatabase() AND name = 'logs'",
    )
    .await;
    let created = one_text(
        &client,
        "SELECT create_table_query AS value FROM system.tables \
         WHERE database = currentDatabase() AND name = 'logs'",
    )
    .await;
    let indexes = one_text(
        &client,
        "SELECT arrayStringConcat(groupArray(concat(expr, ':', type)), ',') AS value \
         FROM system.data_skipping_indices \
         WHERE database = currentDatabase() AND table = 'logs'",
    )
    .await;
    let trace_type = one_text(
        &client,
        "SELECT type AS value FROM system.columns \
         WHERE database = currentDatabase() AND table = 'logs' AND name = 'trace_id'",
    )
    .await;

    assert_eq!(sorting_key, "tenant_id, service_name, time");
    assert_eq!(partition_key, "toDate(time)");
    assert!(created.contains("TTL expire_at"), "{created}");
    assert!(created.contains("ENGINE = MergeTree"), "{created}");
    assert!(indexes.contains("trace_id:bloom_filter"), "{indexes}");
    assert!(indexes.contains("body:tokenbf_v1"), "{indexes}");
    assert_eq!(trace_type, "FixedString(16)");
}

#[tokio::test]
async fn the_metric_tables_lead_with_the_tenant_and_expire_each_row() {
    let Some(client) = fresh_database("ferristrace_migrate_metrics").await else {
        return;
    };
    ClickhouseMigrator::new(client.clone())
        .up()
        .await
        .expect("the migrations apply");
    let described = async |table: &str, column: &str| {
        one_text(
            &client,
            &format!(
                "SELECT {column} AS value FROM system.tables \
                 WHERE database = currentDatabase() AND name = '{table}'"
            ),
        )
        .await
    };

    for table in ["metric_points", "metric_histograms", "metric_exemplars"] {
        assert_eq!(
            described(table, "sorting_key").await,
            "tenant_id, name, time",
            "{table}"
        );
        assert_eq!(described(table, "engine").await, "MergeTree", "{table}");
        assert!(
            described(table, "create_table_query")
                .await
                .contains("TTL expire_at"),
            "{table}"
        );
    }
    assert_eq!(
        described("metric_catalogue", "sorting_key").await,
        "tenant_id, name, project_id"
    );
    assert_eq!(
        described("metric_catalogue", "engine").await,
        "ReplacingMergeTree"
    );
    assert!(
        described("metric_catalogue", "create_table_query")
            .await
            .contains("TTL expire_at")
    );
    let kinds = one_text(
        &client,
        "SELECT type AS value FROM system.columns \
         WHERE database = currentDatabase() AND table = 'metric_points' AND name = 'kind'",
    )
    .await;
    let counts = one_text(
        &client,
        "SELECT type AS value FROM system.columns \
         WHERE database = currentDatabase() AND table = 'metric_histograms' AND name = 'counts'",
    )
    .await;
    assert_eq!(kinds, "Enum8(\'gauge\' = 1, \'sum\' = 2)");
    assert_eq!(counts, "Array(UInt64)");
}
