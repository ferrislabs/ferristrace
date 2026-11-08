use std::collections::BTreeMap;

use clickhouse::Row;
use ferristrace_domain::{
    InfraFilter, InfraReader, PlacementName, Resource, ResourceKind, StoreError, TenantId, Window,
};
use serde::Deserialize;

use crate::clickhouse::metric_read::ClickhouseMetricReader;
use crate::clickhouse::read::{nanos, refused, unsigned, uuid_of};

const MAX_RESOURCES: u32 = 2_000;
const MAX_FIGURES: u32 = 50_000;
const MAX_SERVICES: u32 = 20;
const MAX_SERVED: u32 = 20_000;

const CONTAINER_CPU: &str = "container.cpu.utilization";
const CONTAINER_MEMORY: &str = "container.memory.usage.total";
const CONTAINER_WORKING_SET: &str = "container.memory.working_set";

const RESOURCES: &str = "SELECT \
     kind AS resource_kind, \
     id AS resource_id, \
     max(label) AS resource_name, \
     max(cluster) AS resource_cluster, \
     max(environment) AS resource_environment, \
     max(provider) AS resource_provider, \
     max(region) AS resource_region, \
     max(zone) AS resource_zone, \
     max(host_type) AS resource_host_type, \
     toUnixTimestamp64Nano(min(first_seen)) AS first_nanos, \
     toUnixTimestamp64Nano(max(last_seen)) AS last_nanos \
   FROM infra_resources \
   WHERE tenant_id = ? \
     AND (? = 0 OR project_id = ?) \
     AND (? = '' OR kind = ?) \
   GROUP BY kind, id \
   HAVING (? = '' OR resource_cluster = ?) \
     AND (? = '' OR resource_environment = ?) \
     AND (? = '' OR resource_provider = ?) \
     AND (? = '' OR resource_region = ?) \
   ORDER BY multiIf(kind = 'cluster', 0, kind = 'host', 1, 2), resource_name, resource_id \
   LIMIT {limit}";

const FIGURES: &str = "SELECT \
     coalesce(nullIf(attributes['container.name'], ''), attributes['container.id']) AS resource_id, \
     name AS metric, \
     argMax(value, time) AS latest \
   FROM metric_points \
   WHERE tenant_id = ? \
     AND (? = 0 OR project_id = ?) \
     AND time >= fromUnixTimestamp64Nano(?) AND time < fromUnixTimestamp64Nano(?) \
     AND attributes['k8s.pod.name'] = '' \
     AND name IN ('container.cpu.utilization', 'container.memory.usage.total', 'container.memory.working_set') \
   GROUP BY resource_id, metric \
   LIMIT {limit}";

const SERVED: &str = "SELECT resource_kind, resource_id, groupUniqArray({services})(service_name) AS names FROM ( \
     SELECT 'cluster' AS resource_kind, attributes['k8s.cluster.name'] AS resource_id, service_name FROM spans \
       WHERE {scoped} AND attributes['k8s.cluster.name'] != '' \
     UNION ALL \
     SELECT 'host', coalesce(nullIf(attributes['host.id'], ''), attributes['host.name']), service_name FROM spans \
       WHERE {scoped} AND coalesce(nullIf(attributes['host.id'], ''), attributes['host.name']) != '' \
     UNION ALL \
     SELECT 'container', coalesce(nullIf(attributes['container.name'], ''), attributes['container.id']), service_name FROM spans \
       WHERE {scoped} AND attributes['k8s.pod.name'] = '' \
         AND coalesce(nullIf(attributes['container.name'], ''), attributes['container.id']) != '' \
   ) GROUP BY resource_kind, resource_id LIMIT {limit}";

const SCOPED: &str = "tenant_id = ? AND (? = 0 OR project_id = ?) \
     AND start_time >= fromUnixTimestamp64Nano(?) AND start_time < fromUnixTimestamp64Nano(?)";

#[derive(Debug, Row, Deserialize)]
struct ServedRow {
    resource_kind: String,
    resource_id: String,
    names: Vec<String>,
}

#[derive(Debug, Row, Deserialize)]
struct ResourceRow {
    resource_kind: String,
    resource_id: String,
    resource_name: String,
    resource_cluster: String,
    resource_environment: String,
    resource_provider: String,
    resource_region: String,
    resource_zone: String,
    resource_host_type: String,
    first_nanos: i64,
    last_nanos: i64,
}

#[derive(Debug, Row, Deserialize)]
struct FigureRow {
    resource_id: String,
    metric: String,
    latest: f64,
}

fn text(name: Option<&PlacementName>) -> &str {
    name.map_or("", PlacementName::as_str)
}

#[derive(Debug, Default)]
struct Figures {
    cpu: Option<f64>,
    memory: Option<f64>,
    working_set: Option<f64>,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn bytes(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 0.0).then(|| value.round() as u64)
}

impl ClickhouseMetricReader {
    async fn container_figures(
        &self,
        org: TenantId,
        figures: Window,
        filter: &InfraFilter,
    ) -> Result<BTreeMap<String, Figures>, StoreError> {
        let sql = FIGURES.replace("{limit}", &MAX_FIGURES.to_string());
        let found: Vec<FigureRow> = self
            .client()
            .query(&sql)
            .bind(org.as_uuid())
            .bind(u8::from(filter.project.is_some()))
            .bind(uuid_of(filter.project))
            .bind(nanos(figures.from_unix_nanos()))
            .bind(nanos(figures.to_unix_nanos()))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;
        let mut held: BTreeMap<String, Figures> = BTreeMap::new();
        for row in found {
            let entry = held.entry(row.resource_id).or_default();
            match row.metric.as_str() {
                CONTAINER_CPU => entry.cpu = Some(row.latest),
                CONTAINER_MEMORY => entry.memory = Some(row.latest),
                CONTAINER_WORKING_SET => entry.working_set = Some(row.latest),
                _ => {}
            }
        }
        Ok(held)
    }

    async fn served(
        &self,
        org: TenantId,
        figures: Window,
        filter: &InfraFilter,
    ) -> Result<BTreeMap<(String, String), Vec<String>>, StoreError> {
        let sql = SERVED
            .replace("{scoped}", SCOPED)
            .replace("{services}", &MAX_SERVICES.to_string())
            .replace("{limit}", &MAX_SERVED.to_string());
        let mut query = self.client().query(&sql);
        for _ in 0..3 {
            query = query
                .bind(org.as_uuid())
                .bind(u8::from(filter.project.is_some()))
                .bind(uuid_of(filter.project))
                .bind(nanos(figures.from_unix_nanos()))
                .bind(nanos(figures.to_unix_nanos()));
        }
        let served: Vec<ServedRow> = query.fetch_all().await.map_err(|error| refused(&error))?;
        Ok(served
            .into_iter()
            .map(|row| ((row.resource_kind, row.resource_id), row.names))
            .collect())
    }
}

impl InfraReader for ClickhouseMetricReader {
    async fn resources(
        &self,
        org: TenantId,
        figures: Window,
        filter: &InfraFilter,
    ) -> Result<Vec<Resource>, StoreError> {
        let kind = filter.kind.map_or("", ResourceKind::name);
        let cluster = text(filter.cluster.as_ref());
        let environment = text(filter.environment.as_ref());
        let provider = text(filter.provider.as_ref());
        let region = text(filter.region.as_ref());

        let sql = RESOURCES.replace("{limit}", &MAX_RESOURCES.to_string());
        let rows: Vec<ResourceRow> = self
            .client()
            .query(&sql)
            .bind(org.as_uuid())
            .bind(u8::from(filter.project.is_some()))
            .bind(uuid_of(filter.project))
            .bind(kind)
            .bind(kind)
            .bind(cluster)
            .bind(cluster)
            .bind(environment)
            .bind(environment)
            .bind(provider)
            .bind(provider)
            .bind(region)
            .bind(region)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let held = if rows.iter().any(|row| row.resource_kind == "container") {
            self.container_figures(org, figures, filter).await?
        } else {
            BTreeMap::new()
        };
        let mut running = self.served(org, figures, filter).await?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let kind = ResourceKind::parse(&row.resource_kind)?;
                let seen = if kind == ResourceKind::Container {
                    held.get(&row.resource_id)
                } else {
                    None
                };
                Some(Resource {
                    kind,
                    cpu_percent: seen
                        .and_then(|figures| figures.cpu)
                        .filter(|v| v.is_finite()),
                    memory_bytes: seen
                        .and_then(|figures| figures.memory.or(figures.working_set))
                        .and_then(bytes),
                    services: {
                        let mut names = running
                            .remove(&(row.resource_kind.clone(), row.resource_id.clone()))
                            .unwrap_or_default();
                        names.sort();
                        names.into_iter().map(Into::into).collect()
                    },
                    id: row.resource_id.into(),
                    name: row.resource_name.into(),
                    cluster: row.resource_cluster.into(),
                    environment: row.resource_environment.into(),
                    provider: row.resource_provider.into(),
                    region: row.resource_region.into(),
                    zone: row.resource_zone.into(),
                    host_type: row.resource_host_type.into(),
                    first_seen_unix_nanos: unsigned(row.first_nanos),
                    last_seen_unix_nanos: unsigned(row.last_nanos),
                    silent: false,
                })
            })
            .collect())
    }
}
