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
    Migration {
        version: 7,
        name: "logs",
        up: include_str!("../../migrations/clickhouse/0007_logs.up.sql"),
        down: include_str!("../../migrations/clickhouse/0007_logs.down.sql"),
    },
    Migration {
        version: 8,
        name: "metrics",
        up: include_str!("../../migrations/clickhouse/0008_metrics.up.sql"),
        down: include_str!("../../migrations/clickhouse/0008_metrics.down.sql"),
    },
    Migration {
        version: 9,
        name: "placement",
        up: include_str!("../../migrations/clickhouse/0009_placement.up.sql"),
        down: include_str!("../../migrations/clickhouse/0009_placement.down.sql"),
    },
    Migration {
        version: 10,
        name: "releases",
        up: include_str!("../../migrations/clickhouse/0010_releases.up.sql"),
        down: include_str!("../../migrations/clickhouse/0010_releases.down.sql"),
    },
    Migration {
        version: 11,
        name: "llm_calls",
        up: include_str!("../../migrations/clickhouse/0011_llm_calls.up.sql"),
        down: include_str!("../../migrations/clickhouse/0011_llm_calls.down.sql"),
    },
    Migration {
        version: 12,
        name: "infra_resources",
        up: include_str!("../../migrations/clickhouse/0012_infra_resources.up.sql"),
        down: include_str!("../../migrations/clickhouse/0012_infra_resources.down.sql"),
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
    fn the_logs_key_leads_with_the_tenant_and_expires_each_row() {
        let up = MIGRATIONS
            .iter()
            .find(|migration| migration.name == "logs")
            .expect("a logs migration")
            .up;

        assert!(
            up.contains("ORDER BY (tenant_id, service_name, time)"),
            "{up}"
        );
        assert!(up.contains("PARTITION BY toDate(time)"), "{up}");
        assert!(up.contains("TTL expire_at DELETE"), "{up}");
        assert!(up.contains("trace_id TYPE bloom_filter"), "{up}");
        assert!(up.contains("body TYPE tokenbf_v1"), "{up}");
    }

    #[test]
    fn every_metric_table_leads_with_the_tenant_and_expires_each_row() {
        let up = MIGRATIONS
            .iter()
            .find(|migration| migration.name == "metrics")
            .expect("a metrics migration")
            .up;
        let tables: Vec<&str> = statements(up).collect();

        assert_eq!(tables.len(), 4, "{up}");
        for table in &tables {
            assert!(table.contains("ORDER BY (tenant_id, name"), "{table}");
            assert!(table.contains("TTL expire_at DELETE"), "{table}");
        }
        assert!(tables[3].contains("ReplacingMergeTree(last_seen)"), "{up}");
    }

    #[test]
    fn the_placement_reads_the_resource_of_a_log_and_the_attributes_of_everything_else() {
        let up = MIGRATIONS
            .iter()
            .find(|migration| migration.name == "placement")
            .expect("a placement migration")
            .up;
        let altered: Vec<&str> = statements(up).collect();

        assert_eq!(altered.len(), 4, "{up}");
        for statement in &altered {
            let map = if statement.starts_with("ALTER TABLE logs") {
                "resource"
            } else {
                "attributes"
            };
            for key in [
                "k8s.cluster.name",
                "k8s.namespace.name",
                "k8s.pod.name",
                "k8s.node.name",
                "k8s.deployment.name",
                "k8s.statefulset.name",
                "k8s.daemonset.name",
                "k8s.cronjob.name",
                "k8s.job.name",
            ] {
                assert!(
                    statement.contains(&format!("{map}['{key}']")),
                    "{statement}"
                );
            }
        }
    }

    #[test]
    fn a_release_is_one_aggregated_row_per_tenant_project_service_version_and_environment() {
        let up = MIGRATIONS
            .iter()
            .find(|migration| migration.name == "releases")
            .expect("a releases migration")
            .up;
        let parts: Vec<&str> = statements(up).collect();

        assert_eq!(parts.len(), 4, "{up}");
        assert!(parts[0].contains("ENGINE = AggregatingMergeTree"), "{up}");
        assert!(
            parts[0]
                .contains("ORDER BY (tenant_id, project_id, service_name, version, environment)"),
            "{up}"
        );
        assert!(parts[0].contains("first_seen   SimpleAggregateFunction(min,"));
        assert!(parts[0].contains("last_seen    SimpleAggregateFunction(max,"));
        assert!(parts[0].contains("requests     SimpleAggregateFunction(sum,"));
        assert!(
            parts[2].contains("attributes['service.version'] != ''"),
            "{up}"
        );
        assert!(parts[3].contains("'deploy', 'deployment.event'"), "{up}");
        assert!(parts[3].contains("toUInt64(0) AS requests"), "{up}");
    }

    #[test]
    fn an_llm_call_is_one_row_per_genai_span_keyed_by_tenant_and_time() {
        let up = MIGRATIONS
            .iter()
            .find(|migration| migration.name == "llm_calls")
            .expect("an llm_calls migration")
            .up;
        let parts: Vec<&str> = statements(up).collect();

        assert_eq!(parts.len(), 3, "{up}");
        assert!(parts[0].contains("ORDER BY (tenant_id, time,"), "{up}");
        assert!(parts[0].contains("TTL expire_at DELETE"), "{up}");
        for select in &parts[1..] {
            assert!(
                select.contains("attributes['gen_ai.request.model'] != ''"),
                "{select}"
            );
            assert!(
                select.contains("attributes['gen_ai.usage.cache_read.input_tokens']"),
                "{select}"
            );
        }
        assert!(parts[1].starts_with("INSERT INTO llm_calls"), "{up}");
        assert!(parts[2].contains("MATERIALIZED VIEW"), "{up}");
    }

    #[test]
    fn statements_split_on_semicolons_and_drop_blanks() {
        let parsed: Vec<&str> = statements("CREATE A;\n\n DROP B ;\n").collect();

        assert_eq!(parsed, vec!["CREATE A", "DROP B"]);
    }
}
