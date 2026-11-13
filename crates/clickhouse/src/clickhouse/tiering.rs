use std::num::NonZeroU16;

use clickhouse::{Client, Row};
use serde::Deserialize;

pub const TIERED_POLICY: &str = "tiered";
pub const COLD_VOLUME: &str = "cold";

pub const MOVED_TABLES: &[MovedTable] = &[
    MovedTable::new("spans", "start_time"),
    MovedTable::new("span_events", "time"),
    MovedTable::new("logs", "time"),
    MovedTable::new("metric_points", "time"),
    MovedTable::new("metric_histograms", "time"),
    MovedTable::new("metric_exemplars", "time"),
    MovedTable::new("llm_calls", "time"),
];

const DELETE_ONLY: &str = "expire_at DELETE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovedTable {
    pub name: &'static str,
    time: &'static str,
}

impl MovedTable {
    const fn new(name: &'static str, time: &'static str) -> Self {
        Self { name, time }
    }

    fn ttl(self, cold_after_days: NonZeroU16) -> String {
        format!(
            "toDateTime({}) + INTERVAL {cold_after_days} DAY TO VOLUME '{COLD_VOLUME}', {DELETE_ONLY}",
            self.time
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TieringError {
    #[error("clickhouse refused a tiering statement")]
    Clickhouse(#[from] clickhouse::error::Error),
    #[error(
        "spans.cold_after_days is set but ClickHouse has no storage policy `{0}` \
         with a volume `{COLD_VOLUME}`"
    )]
    MissingPolicy(&'static str),
}

#[derive(Debug, Row, Deserialize)]
struct TablePolicy {
    name: String,
    storage_policy: String,
}

#[derive(Debug, Row, Deserialize)]
struct Volume {
    volume_name: String,
}

#[derive(Debug, Clone)]
pub struct ClickhouseTiering {
    client: Client,
    policy: &'static str,
}

impl ClickhouseTiering {
    pub fn new(client: Client) -> Self {
        Self {
            client: client.with_setting("materialize_ttl_after_modify", "0"),
            policy: TIERED_POLICY,
        }
    }

    pub async fn apply(&self, cold_after_days: Option<NonZeroU16>) -> Result<(), TieringError> {
        let policies = self.table_policies().await?;
        match cold_after_days {
            Some(days) => self.tier(&policies, days).await,
            None => self.untier(&policies).await,
        }
    }

    async fn tier(&self, policies: &[TablePolicy], days: NonZeroU16) -> Result<(), TieringError> {
        if !self.has_cold_volume().await? {
            return Err(TieringError::MissingPolicy(self.policy));
        }
        for table in MOVED_TABLES {
            if policy_of(policies, table.name) != Some(self.policy) {
                self.execute(&format!(
                    "ALTER TABLE {} MODIFY SETTING storage_policy = '{}'",
                    table.name, self.policy
                ))
                .await?;
            }
            self.execute(&format!(
                "ALTER TABLE {} MODIFY TTL {}",
                table.name,
                table.ttl(days)
            ))
            .await?;
        }
        Ok(())
    }

    async fn untier(&self, policies: &[TablePolicy]) -> Result<(), TieringError> {
        for table in MOVED_TABLES {
            if policy_of(policies, table.name) == Some(self.policy) {
                self.execute(&format!(
                    "ALTER TABLE {} MODIFY TTL {DELETE_ONLY}",
                    table.name
                ))
                .await?;
            }
        }
        Ok(())
    }

    async fn table_policies(&self) -> Result<Vec<TablePolicy>, TieringError> {
        Ok(self
            .client
            .query(
                "SELECT name, storage_policy FROM system.tables \
                 WHERE database = currentDatabase()",
            )
            .fetch_all()
            .await?)
    }

    async fn has_cold_volume(&self) -> Result<bool, TieringError> {
        let volumes: Vec<Volume> = self
            .client
            .query("SELECT volume_name FROM system.storage_policies WHERE policy_name = ?")
            .bind(self.policy)
            .fetch_all()
            .await?;
        Ok(volumes
            .iter()
            .any(|volume| volume.volume_name == COLD_VOLUME))
    }

    async fn execute(&self, statement: &str) -> Result<(), TieringError> {
        self.client.query(statement).execute().await?;
        Ok(())
    }
}

fn policy_of<'a>(policies: &'a [TablePolicy], table: &str) -> Option<&'a str> {
    policies
        .iter()
        .find(|policy| policy.name == table)
        .map(|policy| policy.storage_policy.as_str())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use clickhouse::Client;

    use super::{ClickhouseTiering, MOVED_TABLES, MovedTable, TieringError};

    fn days(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).expect("a non-zero day count")
    }

    #[test]
    fn a_span_moves_by_its_start_and_is_deleted_at_its_expiry() {
        assert_eq!(
            MovedTable::new("spans", "start_time").ttl(days(7)),
            "toDateTime(start_time) + INTERVAL 7 DAY TO VOLUME 'cold', expire_at DELETE"
        );
    }

    #[test]
    fn catalogues_stay_on_the_local_volume() {
        let names: Vec<_> = MOVED_TABLES.iter().map(|table| table.name).collect();

        for catalogue in ["metric_catalogue", "releases", "infra_resources"] {
            assert!(!names.contains(&catalogue), "{catalogue} would move");
        }
    }

    #[tokio::test]
    async fn a_missing_policy_stops_the_start_and_is_named() {
        let Some(url) = std::env::var("CLICKHOUSE_URL").ok() else {
            return;
        };
        let mut client = Client::default().with_url(url);
        if let Ok(user) = std::env::var("CLICKHOUSE_USER") {
            client = client.with_user(user);
        }
        if let Ok(password) = std::env::var("CLICKHOUSE_PASSWORD") {
            client = client.with_password(password);
        }
        let tiering = ClickhouseTiering {
            policy: "ferristrace_absent_policy",
            ..ClickhouseTiering::new(client)
        };

        let refused = tiering.apply(Some(days(7))).await;

        let Err(error @ TieringError::MissingPolicy(_)) = refused else {
            panic!("expected a missing policy, got {refused:?}");
        };
        assert!(error.to_string().contains("ferristrace_absent_policy"));
    }
}
