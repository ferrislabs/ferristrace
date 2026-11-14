use std::collections::BTreeMap;

use clickhouse::{Client, Row};
use ferristrace_domain::telemetry::{BillingHour, HourSplit, Split, StoredVolumes};
use ferristrace_domain::{StoreError, TenantId, UnixSeconds};
use serde::Deserialize;
use uuid::Uuid;

use super::read::refused;

const SPANS: &str = "SELECT tenant_id AS tenant, \
     toUnixTimestamp(toStartOfHour(start_time)) AS hour, \
     sum(byteSize(*)) AS volume \
   FROM spans GROUP BY tenant, hour";

const LOGS: &str = "SELECT tenant_id AS tenant, \
     toUnixTimestamp(toStartOfHour(time)) AS hour, \
     sum(byteSize(*)) AS volume \
   FROM logs GROUP BY tenant, hour";

const POINTS: &str = "SELECT tenant_id AS tenant, \
     toUnixTimestamp(toStartOfHour(time)) AS hour, \
     count() AS volume \
   FROM metric_points GROUP BY tenant, hour";

const HISTOGRAMS: &str = "SELECT tenant_id AS tenant, \
     toUnixTimestamp(toStartOfHour(time)) AS hour, \
     count() AS volume \
   FROM metric_histograms GROUP BY tenant, hour";

#[derive(Debug, Row, Deserialize)]
struct VolumeRow {
    #[serde(with = "clickhouse::serde::uuid")]
    tenant: Uuid,
    hour: u32,
    volume: u64,
}

#[derive(Debug, Clone)]
pub struct ClickhouseVolumes {
    client: Client,
}

impl ClickhouseVolumes {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    async fn read(&self, sql: &str) -> Result<Vec<VolumeRow>, StoreError> {
        self.client
            .query(sql)
            .fetch_all::<VolumeRow>()
            .await
            .map_err(|error| refused(&error))
    }
}

impl StoredVolumes for ClickhouseVolumes {
    async fn hourly(&self) -> Result<Vec<HourSplit>, StoreError> {
        let mut hours: BTreeMap<(Uuid, u32), Split> = BTreeMap::new();
        for row in self.read(SPANS).await? {
            hours.entry((row.tenant, row.hour)).or_default().spans_bytes += row.volume;
        }
        for row in self.read(LOGS).await? {
            hours.entry((row.tenant, row.hour)).or_default().logs_bytes += row.volume;
        }
        for sql in [POINTS, HISTOGRAMS] {
            for row in self.read(sql).await? {
                hours
                    .entry((row.tenant, row.hour))
                    .or_default()
                    .metrics_points += row.volume;
            }
        }
        Ok(hours
            .into_iter()
            .map(|((tenant, hour), split)| HourSplit {
                tenant: TenantId::new(tenant),
                hour: BillingHour::containing(UnixSeconds::new(u64::from(hour))),
                split,
            })
            .collect())
    }
}
