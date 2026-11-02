use clickhouse::Row;
use ferristrace_domain::{
    CulpritPod, CulpritReader, Culprits, Labels, MAX_CULPRIT_FACETS, MAX_CULPRIT_PODS, ProjectId,
    Signal, StoreError, TenantId, TraceMeasure, TraceSignal, Window,
};
use serde::Deserialize;

use crate::clickhouse::read::{ClickhouseReader, refused};

const PODS: &str = "SELECT k8s_pod AS value, \
     argMaxIf(k8s_node, start_time, k8s_node != '') AS node, \
     count() AS hits \
   {served}{narrowed} AND k8s_pod != '' \
   GROUP BY k8s_pod ORDER BY hits DESC, value LIMIT {pods}";

const PODS_SEEN: &str = "SELECT uniqExact(k8s_pod) AS total \
   {served}{narrowed} AND k8s_pod != ''";

const NODES: &str = "SELECT k8s_node AS value, '' AS node, count() AS hits \
   {served}{narrowed} AND k8s_node != '' \
   GROUP BY k8s_node ORDER BY hits DESC, value LIMIT {facets}";

const VERSIONS: &str = "SELECT attributes['service.version'] AS value, '' AS node, count() AS hits \
   {served}{narrowed} AND attributes['service.version'] != '' \
   GROUP BY value ORDER BY hits DESC, value LIMIT {facets}";

const ERRORS_ONLY: &str = " AND status = 'error'";
const OPERATION: &str = " AND (? = '' OR name = ?)";

#[derive(Debug, Row, Deserialize)]
struct CulpritRow {
    value: String,
    node: String,
    hits: u64,
}

#[derive(Debug, Row, Deserialize)]
struct SeenRow {
    total: u64,
}

fn sql(template: &str, signal: &TraceSignal) -> String {
    let mut narrowed = String::from(OPERATION);
    if signal.measure == TraceMeasure::ErrorRate {
        narrowed.push_str(ERRORS_ONLY);
    }
    template
        .replace("{narrowed}", &narrowed)
        .replace("{pods}", &MAX_CULPRIT_PODS.to_string())
        .replace("{facets}", &MAX_CULPRIT_FACETS.to_string())
}

impl ClickhouseReader {
    async fn culprit_rows(
        &self,
        template: &str,
        org: TenantId,
        project: Option<ProjectId>,
        signal: &TraceSignal,
        window: Window,
    ) -> Result<Vec<CulpritRow>, StoreError> {
        let filter = signal.filter(project, window);
        let operation = signal.operation.as_deref().unwrap_or_default();
        self.served(
            &sql(template, signal),
            org,
            signal.service.as_deref(),
            &filter,
        )
        .bind(operation)
        .bind(operation)
        .fetch_all()
        .await
        .map_err(|error| refused(&error))
    }

    async fn pods_seen(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        signal: &TraceSignal,
        window: Window,
    ) -> Result<u64, StoreError> {
        let filter = signal.filter(project, window);
        let operation = signal.operation.as_deref().unwrap_or_default();
        let seen: SeenRow = self
            .served(
                &sql(PODS_SEEN, signal),
                org,
                signal.service.as_deref(),
                &filter,
            )
            .bind(operation)
            .bind(operation)
            .fetch_one()
            .await
            .map_err(|error| refused(&error))?;
        Ok(seen.total)
    }
}

impl CulpritReader for ClickhouseReader {
    async fn culprits(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        signal: &Signal,
        window: Window,
    ) -> Result<Culprits, StoreError> {
        let Signal::Traces(traces) = signal else {
            return Ok(Culprits::default());
        };
        let pods = self
            .culprit_rows(PODS, org, project, traces, window)
            .await?;
        let total = self.pods_seen(org, project, traces, window).await?;
        let nodes = self
            .culprit_rows(NODES, org, project, traces, window)
            .await?;
        let versions = self
            .culprit_rows(VERSIONS, org, project, traces, window)
            .await?;
        Ok(Culprits::new(
            pods.into_iter()
                .map(|row| CulpritPod {
                    name: row.value.into(),
                    node: Some(row.node)
                        .filter(|node| !node.is_empty())
                        .map(Into::into),
                    count: row.hits,
                })
                .collect(),
            total,
            nodes.into_iter().map(|row| row.value.into()).collect(),
            versions.into_iter().map(|row| row.value.into()).collect(),
            &Labels::none(),
        ))
    }
}
