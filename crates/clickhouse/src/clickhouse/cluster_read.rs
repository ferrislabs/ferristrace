use std::collections::BTreeMap;

use clickhouse::Row;
use clickhouse::query::Query;
use ferristrace_domain::{
    ClusterFilter, ClusterReader, ClusterSnapshot, FRESH_SECS, Node, Pod, PodPhase, StoreError,
    TenantId, Window, WorkloadKind, WorkloadSummary,
};
use serde::Deserialize;

use crate::clickhouse::metric_read::ClickhouseMetricReader;
use crate::clickhouse::read::{nanos, refused, uuid_of};

const NANOS_PER_SECOND: u64 = 1_000_000_000;
const NANOS_PER_MILLI: u64 = 1_000_000;
const MAX_SERIES: u32 = 200_000;
const MAX_PODS: u32 = 20_000;
const MAX_SERVICES: u32 = 20;
const MAX_PLACES: u32 = 1_000;

const POD_PHASE: &str = "k8s.pod.phase";
const CONTAINER_RESTARTS: &str = "k8s.container.restarts";
const CONTAINER_READY: &str = "k8s.container.ready";
const POD_CPU_USAGE: &str = "k8s.pod.cpu.usage";
const POD_CPU_UTILIZATION: &str = "k8s.pod.cpu.utilization";
const POD_MEMORY: &str = "k8s.pod.memory.working_set";
const NODE_CONDITION_READY: &str = "k8s.node.condition_ready";
const NODE_CONDITION: &str = "k8s.node.condition";
const NODE_CPU_USAGE: &str = "k8s.node.cpu.usage";
const NODE_CPU_UTILIZATION: &str = "k8s.node.cpu.utilization";
const NODE_MEMORY: &str = "k8s.node.memory.working_set";
const NODE_AVAILABLE: &str = "k8s.node.memory.available";

const WORKLOAD_GAUGES: [(&str, WorkloadKind, Gauge); 6] = [
    (
        "k8s.deployment.desired",
        WorkloadKind::Deployment,
        Gauge::Desired,
    ),
    (
        "k8s.deployment.available",
        WorkloadKind::Deployment,
        Gauge::Ready,
    ),
    (
        "k8s.statefulset.desired_pods",
        WorkloadKind::StatefulSet,
        Gauge::Desired,
    ),
    (
        "k8s.statefulset.ready_pods",
        WorkloadKind::StatefulSet,
        Gauge::Ready,
    ),
    (
        "k8s.daemonset.desired_scheduled_nodes",
        WorkloadKind::DaemonSet,
        Gauge::Desired,
    ),
    (
        "k8s.daemonset.ready_nodes",
        WorkloadKind::DaemonSet,
        Gauge::Ready,
    ),
];

const REPORTED: [&str; 12] = [
    POD_PHASE,
    CONTAINER_RESTARTS,
    CONTAINER_READY,
    POD_CPU_USAGE,
    POD_CPU_UTILIZATION,
    POD_MEMORY,
    NODE_CONDITION_READY,
    NODE_CONDITION,
    NODE_CPU_USAGE,
    NODE_CPU_UTILIZATION,
    NODE_MEMORY,
    NODE_AVAILABLE,
];

const POINTS: &str = "SELECT \
     name, \
     k8s_cluster AS cluster, \
     k8s_namespace AS namespace, \
     k8s_pod AS pod, \
     k8s_node AS node, \
     multiIf(attributes['k8s.deployment.name'] != '', 'deployment', \
       attributes['k8s.statefulset.name'] != '', 'statefulset', \
       attributes['k8s.daemonset.name'] != '', 'daemonset', \
       attributes['k8s.cronjob.name'] != '', 'cronjob', \
       attributes['k8s.job.name'] != '', 'job', '') AS workload_kind, \
     k8s_workload AS workload, \
     attributes['condition'] AS condition, \
     argMax(value, time) AS last, \
     min(value) AS low, \
     max(value) AS high, \
     toUnixTimestamp64Nano(max(time)) AS seen_ns \
   FROM metric_points \
   WHERE {scoped} AND name IN ({names}) \
     AND (? = '' OR k8s_namespace = ? OR startsWith(name, 'k8s.node.')) \
   GROUP BY name, cluster, namespace, pod, node, workload_kind, workload, \
     attributes['k8s.container.name'], condition \
   ORDER BY name, seen_ns \
   LIMIT {limit}";

const PLACES: &str = "SELECT k8s_cluster AS cluster, k8s_namespace AS namespace \
   FROM metric_points \
   WHERE tenant_id = ? AND (? = 0 OR project_id = ?) \
     AND time >= fromUnixTimestamp64Nano(?) AND time < fromUnixTimestamp64Nano(?) \
     AND name IN ({names}) \
   GROUP BY cluster, namespace \
   ORDER BY cluster, namespace \
   LIMIT {limit}";

const SERVICES: &str = "SELECT \
     k8s_cluster AS cluster, \
     k8s_namespace AS namespace, \
     k8s_pod AS pod, \
     groupUniqArray({services})(service_name) AS services \
   FROM spans \
   WHERE tenant_id = ? AND (? = 0 OR project_id = ?) \
     AND start_time >= fromUnixTimestamp64Nano(?) AND start_time < fromUnixTimestamp64Nano(?) \
     AND k8s_pod != '' \
     AND (? = '' OR k8s_cluster = ?) \
     AND (? = '' OR k8s_namespace = ?) \
   GROUP BY cluster, namespace, pod \
   LIMIT {limit}";

const SCOPED: &str = "tenant_id = ? AND (? = 0 OR project_id = ?) \
     AND time >= fromUnixTimestamp64Nano(?) AND time < fromUnixTimestamp64Nano(?) \
     AND (? = '' OR k8s_cluster = ?)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gauge {
    Desired,
    Ready,
}

#[derive(Debug, Row, Deserialize)]
struct PointRow {
    name: String,
    cluster: String,
    namespace: String,
    pod: String,
    node: String,
    workload_kind: String,
    workload: String,
    condition: String,
    last: f64,
    low: f64,
    high: f64,
    seen_ns: i64,
}

#[derive(Debug, Row, Deserialize)]
struct PlaceRow {
    cluster: String,
    namespace: String,
}

#[derive(Debug, Row, Deserialize)]
struct ServedRow {
    cluster: String,
    namespace: String,
    pod: String,
    services: Vec<String>,
}

fn names() -> String {
    REPORTED
        .iter()
        .copied()
        .chain(WORKLOAD_GAUGES.iter().map(|(name, _, _)| *name))
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn fresh(window: Window) -> (u64, u64) {
    let to = window.to_unix_nanos();
    let from = to
        .saturating_sub(FRESH_SECS * NANOS_PER_SECOND)
        .max(window.from_unix_nanos());
    (from, to)
}

fn text(name: Option<&ferristrace_domain::PlacementName>) -> &str {
    name.map_or("", ferristrace_domain::PlacementName::as_str)
}

fn bound(query: Query, org: TenantId, filter: &ClusterFilter, (from, to): (u64, u64)) -> Query {
    query
        .bind(org.as_uuid())
        .bind(u8::from(filter.project.is_some()))
        .bind(uuid_of(filter.project))
        .bind(nanos(from))
        .bind(nanos(to))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn count(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        value.round() as u64
    } else {
        0
    }
}

fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn non_empty(text: String) -> Option<Box<str>> {
    (!text.is_empty()).then(|| text.into_boxed_str())
}

type PodKey = (String, String, String);
type WorkloadKey = (String, String, WorkloadKind, String);

#[derive(Debug, Default)]
struct Gathered {
    pods: BTreeMap<PodKey, Pod>,
    nodes: BTreeMap<String, Node>,
    workloads: BTreeMap<WorkloadKey, WorkloadSummary>,
    seen_ns: i64,
}

impl Gathered {
    fn take(&mut self, row: PointRow) {
        self.seen_ns = self.seen_ns.max(row.seen_ns);
        if let Some((_, kind, gauge)) = WORKLOAD_GAUGES
            .iter()
            .find(|(name, _, _)| *name == row.name)
        {
            self.workload(&row, *kind, *gauge);
        } else if row.name.starts_with("k8s.node.") {
            self.node(&row);
        } else if !row.pod.is_empty() {
            self.pod(row);
        }
    }

    fn workload(&mut self, row: &PointRow, kind: WorkloadKind, gauge: Gauge) {
        if row.workload.is_empty() {
            return;
        }
        let summary = self
            .workloads
            .entry((
                row.cluster.clone(),
                row.namespace.clone(),
                kind,
                row.workload.clone(),
            ))
            .or_insert_with(|| WorkloadSummary {
                cluster: row.cluster.as_str().into(),
                kind,
                name: row.workload.as_str().into(),
                namespace: row.namespace.as_str().into(),
                desired: None,
                ready: None,
                pods: 0,
            });
        let value = Some(count(row.last));
        match gauge {
            Gauge::Desired => summary.desired = value,
            Gauge::Ready => summary.ready = value,
        }
    }

    fn node(&mut self, row: &PointRow) {
        if row.node.is_empty() {
            return;
        }
        let node = self
            .nodes
            .entry(row.node.clone())
            .or_insert_with(|| Node::named(&row.node));
        match row.name.as_str() {
            NODE_CONDITION_READY => node.ready = Some(row.last >= 1.0),
            NODE_CONDITION if row.condition.eq_ignore_ascii_case("ready") => {
                node.ready = Some(row.last >= 1.0);
            }
            NODE_CPU_USAGE => node.cpu_cores = finite(row.last),
            NODE_CPU_UTILIZATION if node.cpu_cores.is_none() => node.cpu_cores = finite(row.last),
            NODE_MEMORY => node.memory_bytes = Some(count(row.last)),
            NODE_AVAILABLE => node.memory_available = Some(count(row.last)),
            _ => {}
        }
    }

    fn pod(&mut self, row: PointRow) {
        let pod = self
            .pods
            .entry((row.cluster.clone(), row.namespace.clone(), row.pod.clone()))
            .or_insert_with(|| Pod {
                cluster: row.cluster.as_str().into(),
                ..Pod::named(&row.pod, &row.namespace)
            });
        if let Some(kind) = WorkloadKind::parse(&row.workload_kind)
            && !row.workload.is_empty()
        {
            pod.workload = Some((kind, row.workload.as_str().into()));
        }
        match row.name.as_str() {
            POD_PHASE => {
                pod.phase = PodPhase::of_value(row.last).or(pod.phase);
            }
            CONTAINER_RESTARTS => {
                pod.restarts_in_window += count(row.high - row.low);
                pod.restarts_total += count(row.last);
            }
            CONTAINER_READY => {
                pod.ready = Some(pod.ready.unwrap_or(true) && row.last >= 1.0);
            }
            POD_CPU_USAGE => pod.cpu_cores = finite(row.last),
            POD_CPU_UTILIZATION if pod.cpu_cores.is_none() => pod.cpu_cores = finite(row.last),
            POD_MEMORY => pod.memory_bytes = Some(count(row.last)),
            _ => {}
        }
        if let Some(node) = non_empty(row.node) {
            pod.node = Some(node);
        }
    }

    fn serve(&mut self, row: ServedRow) {
        let services = row.services.into_iter().map(String::into_boxed_str);
        let key = (row.cluster, row.namespace, row.pod);
        if let Some(pod) = self.pods.get_mut(&key) {
            pod.services.extend(services);
            return;
        }
        if !key.0.is_empty() {
            return;
        }
        if let Some(pod) = self
            .pods
            .values_mut()
            .find(|pod| *pod.namespace == *key.1 && *pod.name == *key.2)
        {
            pod.services.extend(services);
        }
    }
}

impl ClusterReader for ClickhouseMetricReader {
    async fn snapshot(
        &self,
        org: TenantId,
        window: Window,
        filter: &ClusterFilter,
    ) -> Result<ClusterSnapshot, StoreError> {
        let cluster = text(filter.cluster.as_ref());
        let namespace = text(filter.namespace.as_ref());
        let names = names();
        let fresh = fresh(window);

        let points_sql = POINTS
            .replace("{scoped}", SCOPED)
            .replace("{names}", &names)
            .replace("{limit}", &MAX_SERIES.to_string());
        let points: Vec<PointRow> = bound(self.client().query(&points_sql), org, filter, fresh)
            .bind(cluster)
            .bind(cluster)
            .bind(namespace)
            .bind(namespace)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let places_sql = PLACES
            .replace("{names}", &names)
            .replace("{limit}", &MAX_PLACES.to_string());
        let places: Vec<PlaceRow> = bound(self.client().query(&places_sql), org, filter, fresh)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let served_sql = SERVICES
            .replace("{services}", &MAX_SERVICES.to_string())
            .replace("{limit}", &MAX_PODS.to_string());
        let served: Vec<ServedRow> = bound(
            self.client().query(&served_sql),
            org,
            filter,
            (window.from_unix_nanos(), window.to_unix_nanos()),
        )
        .bind(cluster)
        .bind(cluster)
        .bind(namespace)
        .bind(namespace)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))?;

        let mut gathered = Gathered::default();
        for row in points {
            gathered.take(row);
        }
        for row in served {
            gathered.serve(row);
        }
        let seen_ms = (gathered.seen_ns > 0)
            .then(|| u64::try_from(gathered.seen_ns).unwrap_or_default() / NANOS_PER_MILLI);
        let mut namespaces = Vec::new();
        let mut clusters = Vec::new();
        for place in places {
            if cluster.is_empty() || place.cluster == cluster {
                namespaces.extend(non_empty(place.namespace));
            }
            clusters.extend(non_empty(place.cluster));
        }
        Ok(ClusterSnapshot {
            clusters,
            namespaces,
            nodes: gathered.nodes.into_values().collect(),
            workloads: gathered.workloads.into_values().collect(),
            pods: gathered.pods.into_values().collect(),
            seen_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::Window;

    use super::{NANOS_PER_SECOND, fresh, names};

    #[test]
    fn every_receiver_metric_is_asked_for_by_its_literal_name() {
        let listed = names();

        for name in [
            "'k8s.pod.phase'",
            "'k8s.container.restarts'",
            "'k8s.pod.cpu.usage'",
            "'k8s.pod.cpu.utilization'",
            "'k8s.node.condition_ready'",
            "'k8s.node.condition'",
            "'k8s.deployment.available'",
            "'k8s.daemonset.ready_nodes'",
        ] {
            assert!(listed.contains(name), "{name} in {listed}");
        }
    }

    #[test]
    fn latest_values_never_reach_back_past_ten_minutes() {
        let hour = Window::new(0, 3_600 * NANOS_PER_SECOND).expect("an hour");
        let minute = Window::new(0, 60 * NANOS_PER_SECOND).expect("a minute");

        assert_eq!(
            fresh(hour),
            (3_000 * NANOS_PER_SECOND, 3_600 * NANOS_PER_SECOND)
        );
        assert_eq!(fresh(minute), (0, 60 * NANOS_PER_SECOND));
    }
}
