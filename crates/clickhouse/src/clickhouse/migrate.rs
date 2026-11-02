use clickhouse::{Client, Row};
use serde::Deserialize;

const VERSIONS_TABLE: &str = "schema_migrations";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    up: &'static str,
    down: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "spans",
        up: include_str!("../../migrations/clickhouse/0001_spans.up.sql"),
        down: include_str!("../../migrations/clickhouse/0001_spans.down.sql"),
    },
    Migration {
        version: 2,
        name: "trace_lookup",
        up: include_str!("../../migrations/clickhouse/0002_trace_lookup.up.sql"),
        down: include_str!("../../migrations/clickhouse/0002_trace_lookup.down.sql"),
    },
    Migration {
        version: 3,
        name: "span_project",
        up: include_str!("../../migrations/clickhouse/0003_span_project.up.sql"),
        down: include_str!("../../migrations/clickhouse/0003_span_project.down.sql"),
    },
    Migration {
        version: 4,
        name: "span_events",
        up: include_str!("../../migrations/clickhouse/0004_span_events.up.sql"),
        down: include_str!("../../migrations/clickhouse/0004_span_events.down.sql"),
    },
    Migration {
        version: 5,
        name: "replacing_spans",
        up: include_str!("../../migrations/clickhouse/0005_replacing_spans.up.sql"),
        down: include_str!("../../migrations/clickhouse/0005_replacing_spans.down.sql"),
    },
    Migration {
        version: 6,
        name: "event_fingerprint",
        up: include_str!("../../migrations/clickhouse/0006_event_fingerprint.up.sql"),
        down: include_str!("../../migrations/clickhouse/0006_event_fingerprint.down.sql"),
    },
];

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("clickhouse refused a migration statement")]
    Clickhouse(#[from] clickhouse::error::Error),
}

#[derive(Debug, Row, Deserialize)]
struct VersionRow {
    version: u32,
}

#[derive(Debug, Clone)]
pub struct ClickhouseMigrator {
    client: Client,
}

impl ClickhouseMigrator {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn applied(&self) -> Result<Vec<u32>, MigrateError> {
        self.create_versions_table().await?;
        let rows: Vec<VersionRow> = self
            .client
            .query(&format!(
                "SELECT version FROM {VERSIONS_TABLE} FINAL ORDER BY version"
            ))
            .fetch_all()
            .await?;
        Ok(rows.into_iter().map(|row| row.version).collect())
    }

    pub async fn up(&self) -> Result<Vec<u32>, MigrateError> {
        let applied = self.applied().await?;
        let mut ran = Vec::new();
        for migration in MIGRATIONS {
            if applied.contains(&migration.version) {
                continue;
            }
            self.execute(migration.up).await?;
            self.record(*migration).await?;
            ran.push(migration.version);
        }
        Ok(ran)
    }

    pub async fn down_to(&self, floor: u32) -> Result<Vec<u32>, MigrateError> {
        let applied = self.applied().await?;
        let mut reverted = Vec::new();
        for migration in MIGRATIONS.iter().rev() {
            if migration.version <= floor || !applied.contains(&migration.version) {
                continue;
            }
            self.execute(migration.down).await?;
            self.forget(migration.version).await?;
            reverted.push(migration.version);
        }
        Ok(reverted)
    }

    async fn execute(&self, sql: &str) -> Result<(), MigrateError> {
        for statement in statements(sql) {
            self.client.query(statement).execute().await?;
        }
        Ok(())
    }

    async fn create_versions_table(&self) -> Result<(), MigrateError> {
        self.client
            .query(&format!(
                "CREATE TABLE IF NOT EXISTS {VERSIONS_TABLE} \
                 (version UInt32, name String, \
                 applied_at DateTime64(3, 'UTC') DEFAULT now64(3)) \
                 ENGINE = ReplacingMergeTree ORDER BY version"
            ))
            .execute()
            .await?;
        Ok(())
    }

    async fn record(&self, migration: Migration) -> Result<(), MigrateError> {
        self.client
            .query(&format!(
                "INSERT INTO {VERSIONS_TABLE} (version, name) VALUES (?, ?)"
            ))
            .bind(migration.version)
            .bind(migration.name)
            .execute()
            .await?;
        Ok(())
    }

    async fn forget(&self, version: u32) -> Result<(), MigrateError> {
        self.client
            .query(&format!(
                "ALTER TABLE {VERSIONS_TABLE} DELETE WHERE version = ? \
                 SETTINGS mutations_sync = 2"
            ))
            .bind(version)
            .execute()
            .await?;
        Ok(())
    }
}

fn statements(sql: &str) -> impl Iterator<Item = &str> {
    sql.split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{MIGRATIONS, statements};

    #[test]
    fn versions_rise_strictly_and_start_at_one() {
        let versions: Vec<u32> = MIGRATIONS.iter().map(|m| m.version).collect();
        let mut sorted = versions.clone();
        sorted.sort_unstable();
        sorted.dedup();

        assert_eq!(versions, sorted);
        assert_eq!(versions.first(), Some(&1));
    }

    #[test]
    fn every_migration_carries_both_directions() {
        for migration in MIGRATIONS {
            assert!(
                statements(migration.up).count() >= 1,
                "{} has no up statement",
                migration.name
            );
            assert!(
                statements(migration.down).count() >= 1,
                "{} has no down statement",
                migration.name
            );
        }
    }

    #[test]
    fn the_spans_key_leads_with_the_tenant() {
        let up = MIGRATIONS[0].up;

        assert!(up.contains("ORDER BY (tenant_id,"), "{up}");
        assert!(up.contains("PARTITION BY toDate(start_time)"), "{up}");
        assert!(up.contains("TTL expire_at DELETE"), "{up}");
        assert!(up.contains("attributes       Map(String, String)"), "{up}");
    }

    #[test]
    fn statements_split_on_semicolons_and_drop_blanks() {
        let parsed: Vec<&str> = statements("CREATE A;\n\n DROP B ;\n").collect();

        assert_eq!(parsed, vec!["CREATE A", "DROP B"]);
    }
}
