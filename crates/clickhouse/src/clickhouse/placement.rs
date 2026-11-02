use clickhouse::query::Query;
use clickhouse::{Client, Row, RowOwned, RowRead};
use ferristrace_domain::{
    Placement, PlacementReader, Placements, ProjectId, SeenPod, SeenWorkload, Sighting, StoreError,
    TenantId, Window, WorkloadKind,
};
use serde::Deserialize;

use crate::clickhouse::read::{ClickhouseReader, held, nanos, refused, uuid_of};

const SOURCES: [(&str, &str, &str); 4] = [
    ("spans", "attributes", "start_time"),
    ("logs", "resource", "time"),
    ("metric_points", "attributes", "time"),
    ("metric_histograms", "attributes", "time"),
];

const SOURCE: &str = "SELECT {columns} FROM {table} \
   WHERE tenant_id = ? AND (? = 0 OR project_id = ?) \
     AND {time} >= fromUnixTimestamp64Nano(?) AND {time} < fromUnixTimestamp64Nano(?) \
     AND {seen}";

const FACETS: &str = "SELECT pair.1 AS facet, pair.2 AS value FROM ( \
     SELECT arrayJoin([('cluster', k8s_cluster), ('namespace', k8s_namespace), ('node', k8s_node)]) AS pair \
     FROM ({sources}) \
   ) \
   WHERE value != '' \
   GROUP BY facet, value \
   ORDER BY facet, value \
   LIMIT {cap} BY facet";

const WORKLOADS: &str = "SELECT owner.1 AS kind, owner.2 AS name, namespace FROM ( \
     SELECT arrayJoin(arrayFilter(candidate -> candidate.2 != '', owners)) AS owner, namespace \
     FROM ({sources}) \
   ) \
   GROUP BY kind, name, namespace \
   ORDER BY namespace, kind, name \
   LIMIT {cap}";

const PODS: &str = "SELECT name, namespace, argMax(node, seen) AS node \
   FROM ({sources}) \
   GROUP BY name, namespace \
   ORDER BY namespace, name \
   LIMIT {cap}";

const SPANS_PLACED: &str = "SELECT {columns} FROM spans \
   WHERE tenant_id = ? \
     AND start_time >= fromUnixTimestamp64Nano(?) AND start_time < fromUnixTimestamp64Nano(?) \
     AND (? = 0 OR project_id = ?){placed}";

const SIGHTED_ENVIRONMENTS: &str = "SELECT DISTINCT environment FROM ( \
     SELECT if(attributes['deployment.environment.name'] != '', \
               attributes['deployment.environment.name'], \
               attributes['deployment.environment']) AS environment \
     FROM spans \
     WHERE tenant_id = ? AND service_name = ? \
       AND start_time >= fromUnixTimestamp64Nano(?) AND start_time < fromUnixTimestamp64Nano(?) \
       AND (? = 0 OR project_id = ?) \
   ) \
   WHERE environment != '' \
   ORDER BY environment \
   LIMIT 50";

const SIGHTED_PROJECT: &str = "SELECT project_id FROM spans \
   WHERE tenant_id = ? AND service_name = ? \
     AND start_time >= fromUnixTimestamp64Nano(?) AND start_time < fromUnixTimestamp64Nano(?) \
     AND (? = 0 OR project_id = ?) \
   ORDER BY start_time DESC \
   LIMIT 1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Held {
    Attributes,
    Resource,
}

impl Held {
    fn column(self) -> &'static str {
        match self {
            Self::Attributes => "attributes",
            Self::Resource => "resource",
        }
    }
}

#[derive(Debug)]
pub(crate) struct Placed<'a> {
    clauses: Vec<String>,
    values: Vec<&'a str>,
}

impl<'a> Placed<'a> {
    pub(crate) fn of(placement: &'a Placement, held: Held) -> Self {
        let mut placed = Self {
            clauses: Vec::new(),
            values: Vec::new(),
        };
        for (column, name) in [
            ("k8s_cluster", placement.cluster.as_ref()),
            ("k8s_namespace", placement.namespace.as_ref()),
            ("k8s_pod", placement.pod.as_ref()),
            ("k8s_node", placement.node.as_ref()),
        ] {
            if let Some(name) = name {
                placed.clauses.push(format!("{column} = ?"));
                placed.values.push(name.as_str());
            }
        }
        if let Some(workload) = &placement.workload {
            placed.clauses.push(match workload.kind() {
                None => "k8s_workload = ?".to_owned(),
                Some(kind) => format!("{}['{}'] = ?", held.column(), kind.key()),
            });
            placed.values.push(workload.name().as_str());
        }
        placed
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    pub(crate) fn clauses(&self) -> &[String] {
        &self.clauses
    }

    pub(crate) fn values(&self) -> &[&'a str] {
        &self.values
    }

    pub(crate) fn and(&self) -> String {
        self.clauses
            .iter()
            .fold(String::new(), |mut joined, clause| {
                joined.push_str(" AND ");
                joined.push_str(clause);
                joined
            })
    }

    pub(crate) fn bind(&self, mut query: Query) -> Query {
        for value in &self.values {
            query = query.bind(*value);
        }
        query
    }

    pub(crate) fn spans(&self, columns: &str) -> String {
        SPANS_PLACED
            .replace("{columns}", columns)
            .replace("{placed}", &self.and())
    }

    pub(crate) fn bind_spans(
        &self,
        query: Query,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Query {
        self.bind(
            query
                .bind(tenant.as_uuid())
                .bind(nanos(window.from_unix_nanos()))
                .bind(nanos(window.to_unix_nanos()))
                .bind(u8::from(project.is_some()))
                .bind(uuid_of(project)),
        )
    }
}

fn owners(held: &str) -> String {
    let pairs: Vec<String> = WorkloadKind::ALL
        .iter()
        .map(|kind| format!("('{}', {held}['{}'])", kind.name(), kind.key()))
        .collect();
    format!("[{}] AS owners", pairs.join(", "))
}

fn sourced(template: &str, columns: impl Fn(&str) -> String, seen: &str) -> String {
    let sources: Vec<String> = SOURCES
        .iter()
        .map(|(table, held, time)| {
            SOURCE
                .replace("{columns}", &columns(held).replace("{time}", time))
                .replace("{table}", table)
                .replace("{time}", time)
                .replace("{seen}", seen)
        })
        .collect();
    template
        .replace("{sources}", &sources.join(" UNION ALL "))
        .replace("{cap}", &ferristrace_domain::MAX_PLACEMENTS.to_string())
}

fn scoped(mut query: Query, tenant: TenantId, window: Window, project: Option<ProjectId>) -> Query {
    for _ in SOURCES {
        query = query
            .bind(tenant.as_uuid())
            .bind(u8::from(project.is_some()))
            .bind(uuid_of(project))
            .bind(nanos(window.from_unix_nanos()))
            .bind(nanos(window.to_unix_nanos()));
    }
    query
}

#[derive(Debug, Row, Deserialize)]
struct EnvironmentRow {
    environment: String,
}

#[derive(Debug, Row, Deserialize)]
struct ProjectRow {
    #[serde(with = "clickhouse::serde::uuid")]
    project_id: uuid::Uuid,
}

#[derive(Debug, Row, Deserialize)]
struct FacetRow {
    facet: String,
    value: String,
}

#[derive(Debug, Row, Deserialize)]
struct WorkloadRow {
    kind: String,
    name: String,
    namespace: String,
}

#[derive(Debug, Row, Deserialize)]
struct PodRow {
    name: String,
    namespace: String,
    node: String,
}

async fn fetched<T>(
    client: &Client,
    sql: &str,
    tenant: TenantId,
    window: Window,
    project: Option<ProjectId>,
) -> Result<Vec<T>, StoreError>
where
    T: RowOwned + RowRead,
{
    scoped(client.query(sql), tenant, window, project)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))
}

impl PlacementReader for ClickhouseReader {
    async fn sighting(
        &self,
        tenant: TenantId,
        service: &str,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Sighting, StoreError> {
        let client = self.client();
        let bound = |sql: &str| {
            client
                .query(sql)
                .bind(tenant.as_uuid())
                .bind(service)
                .bind(nanos(window.from_unix_nanos()))
                .bind(nanos(window.to_unix_nanos()))
                .bind(u8::from(project.is_some()))
                .bind(uuid_of(project))
        };
        let environments: Vec<EnvironmentRow> = bound(SIGHTED_ENVIRONMENTS)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;
        let latest: Option<ProjectRow> = bound(SIGHTED_PROJECT)
            .fetch_optional()
            .await
            .map_err(|error| refused(&error))?;
        Ok(Sighting {
            project: latest
                .map(|row| row.project_id)
                .filter(|id| !id.is_nil())
                .map(ProjectId::new),
            environments: environments
                .into_iter()
                .map(|row| row.environment.into())
                .collect(),
        })
    }

    async fn placements(
        &self,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Placements, StoreError> {
        let client = self.client();
        let facets: Vec<FacetRow> = fetched(
            client,
            &sourced(
                FACETS,
                |_| "k8s_cluster, k8s_namespace, k8s_node".to_owned(),
                "(k8s_cluster != '' OR k8s_namespace != '' OR k8s_node != '')",
            ),
            tenant,
            window,
            project,
        )
        .await?;
        let workloads: Vec<WorkloadRow> = fetched(
            client,
            &sourced(
                WORKLOADS,
                |held| format!("{}, k8s_namespace AS namespace", owners(held)),
                "k8s_workload != ''",
            ),
            tenant,
            window,
            project,
        )
        .await?;
        let pods: Vec<PodRow> = fetched(
            client,
            &sourced(
                PODS,
                |_| {
                    "k8s_pod AS name, k8s_namespace AS namespace, k8s_node AS node, {time} AS seen"
                        .to_owned()
                },
                "k8s_pod != ''",
            ),
            tenant,
            window,
            project,
        )
        .await?;

        let mut seen = Placements::default();
        for row in facets {
            let kept = match row.facet.as_str() {
                "cluster" => &mut seen.clusters,
                "namespace" => &mut seen.namespaces,
                _ => &mut seen.nodes,
            };
            kept.push(row.value.into());
        }
        for row in workloads {
            seen.workloads.push(SeenWorkload {
                kind: WorkloadKind::parse(&row.kind).ok_or_else(|| held(&row.kind))?,
                name: row.name.into(),
                namespace: row.namespace.into(),
            });
        }
        seen.pods = pods
            .into_iter()
            .map(|row| SeenPod {
                name: row.name.into(),
                namespace: row.namespace.into(),
                node: row.node.into(),
            })
            .collect();
        Ok(seen)
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{Placement, PlacementName, Workload};

    use super::{Held, Placed};

    fn named(text: &str) -> PlacementName {
        PlacementName::try_from(text).expect("a name")
    }

    #[test]
    fn an_empty_placement_adds_no_clause() {
        let placement = Placement::default();
        let placed = Placed::of(&placement, Held::Attributes);

        assert!(placed.is_empty());
        assert_eq!(placed.and(), "");
    }

    #[test]
    fn every_part_of_a_placement_is_bound_never_spliced() {
        let placement = Placement {
            cluster: Some(named("prod")),
            namespace: Some(named("shop")),
            workload: Some(Workload::try_from("checkout").expect("a workload")),
            pod: Some(named("checkout-1")),
            node: Some(named("node-a")),
        };
        let placed = Placed::of(&placement, Held::Attributes);

        assert_eq!(
            placed.and(),
            " AND k8s_cluster = ? AND k8s_namespace = ? AND k8s_pod = ? AND k8s_node = ? \
             AND k8s_workload = ?"
        );
        assert_eq!(
            placed.values(),
            ["prod", "shop", "checkout-1", "node-a", "checkout"]
        );
    }

    #[test]
    fn a_kinded_workload_reads_its_own_key_from_the_map_the_table_keeps_it_in() {
        let placement = Placement {
            workload: Some(Workload::try_from("cronjob:backup").expect("a workload")),
            ..Placement::default()
        };

        assert_eq!(
            Placed::of(&placement, Held::Attributes).clauses(),
            ["attributes['k8s.cronjob.name'] = ?"]
        );
        assert_eq!(
            Placed::of(&placement, Held::Resource).clauses(),
            ["resource['k8s.cronjob.name'] = ?"]
        );
    }
}
