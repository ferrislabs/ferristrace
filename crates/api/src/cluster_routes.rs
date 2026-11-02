use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::ReadRepos;
use ferristrace_domain::{
    ClusterFilter, ClusterSnapshot, ExploreError, Node, PlacementName, Pod, PodPhase, PodReason,
    ProjectId, TenantId, WorkloadSummary,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, reading};
use crate::placement::Misplaced;
use crate::state::AppState;

pub fn cluster_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: ReadRepos,
{
    Router::new()
        .route("/api/orgs/{org}/cluster", get(cluster::<S>))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct Looked {
    #[serde(default)]
    cluster: Option<String>,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    project: Option<Uuid>,
}

fn named(text: Option<&str>, field: &'static str) -> Result<Option<PlacementName>, Misplaced> {
    text.map(str::trim)
        .filter(|held| !held.is_empty())
        .map(|held| PlacementName::try_from(held).map_err(|_| Misplaced(field)))
        .transpose()
}

impl Looked {
    fn filter(&self) -> Result<ClusterFilter, Misplaced> {
        Ok(ClusterFilter {
            cluster: named(self.cluster.as_deref(), "cluster")?,
            namespace: named(self.namespace.as_deref(), "namespace")?,
            project: self.project.map(ProjectId::new),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct Shown {
    clusters: Vec<String>,
    namespaces: Vec<String>,
    nodes: Vec<ShownNode>,
    workloads: Vec<ShownWorkload>,
    pods: Vec<ShownPod>,
    seen_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ShownNode {
    name: String,
    ready: Option<bool>,
    cpu: Option<f64>,
    memory: Option<u64>,
    memory_available: Option<u64>,
    pods: u64,
}

#[derive(Debug, Serialize)]
struct ShownWorkload {
    cluster: Option<String>,
    kind: &'static str,
    name: String,
    namespace: String,
    desired: Option<u64>,
    ready: Option<u64>,
    pods: u64,
}

#[derive(Debug, Serialize)]
struct Owner {
    kind: &'static str,
    name: String,
}

#[derive(Debug, Serialize)]
struct ShownPod {
    name: String,
    namespace: String,
    cluster: Option<String>,
    workload: Option<Owner>,
    node: Option<String>,
    phase: Option<&'static str>,
    ready: Option<bool>,
    restarts: u64,
    restarts_total: u64,
    cpu: Option<f64>,
    memory: Option<u64>,
    services: Vec<String>,
    unhealthy: bool,
    reason: Option<&'static str>,
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|number| number.is_finite())
}

impl From<Node> for ShownNode {
    fn from(node: Node) -> Self {
        Self {
            name: node.name.into_string(),
            ready: node.ready,
            cpu: finite(node.cpu_cores),
            memory: node.memory_bytes,
            memory_available: node.memory_available,
            pods: node.pods,
        }
    }
}

impl From<WorkloadSummary> for ShownWorkload {
    fn from(workload: WorkloadSummary) -> Self {
        Self {
            cluster: Some(workload.cluster.into_string()).filter(|cluster| !cluster.is_empty()),
            kind: workload.kind.name(),
            name: workload.name.into_string(),
            namespace: workload.namespace.into_string(),
            desired: workload.desired,
            ready: workload.ready,
            pods: workload.pods,
        }
    }
}

impl From<Pod> for ShownPod {
    fn from(pod: Pod) -> Self {
        Self {
            name: pod.name.into_string(),
            namespace: pod.namespace.into_string(),
            cluster: Some(pod.cluster.into_string()).filter(|cluster| !cluster.is_empty()),
            workload: pod.workload.map(|(kind, name)| Owner {
                kind: kind.name(),
                name: name.into_string(),
            }),
            node: pod.node.map(str::into_string),
            phase: pod.phase.map(PodPhase::name),
            ready: pod.ready,
            restarts: pod.restarts_in_window,
            restarts_total: pod.restarts_total,
            cpu: finite(pod.cpu_cores),
            memory: pod.memory_bytes,
            services: pod.services.into_iter().map(str::into_string).collect(),
            unhealthy: pod.unhealthy,
            reason: pod.reason.map(PodReason::name),
        }
    }
}

fn strings(held: Vec<Box<str>>) -> Vec<String> {
    held.into_iter().map(str::into_string).collect()
}

impl From<ClusterSnapshot> for Shown {
    fn from(snapshot: ClusterSnapshot) -> Self {
        Self {
            clusters: strings(snapshot.clusters),
            namespaces: strings(snapshot.namespaces),
            nodes: snapshot.nodes.into_iter().map(Into::into).collect(),
            workloads: snapshot.workloads.into_iter().map(Into::into).collect(),
            pods: snapshot.pods.into_iter().map(Into::into).collect(),
            seen_ms: snapshot.seen_ms,
        }
    }
}

async fn cluster<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(looked): Query<Looked>,
    headers: HeaderMap,
) -> Result<Json<Shown>, ClusterRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let filter = looked.filter()?;
    let snapshot = state.app.cluster(&who, TenantId::new(org), &filter).await?;
    Ok(Json(snapshot.into()))
}

impl From<NotCarried> for ClusterRefused {
    fn from(error: NotCarried) -> Self {
        Self::Reaching(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClusterRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error(transparent)]
    Placement(#[from] Misplaced),
    #[error("the cluster could not be read")]
    Unavailable,
}

impl From<ExploreError> for ClusterRefused {
    fn from(error: ExploreError) -> Self {
        match error {
            ExploreError::NoSession => Self::Reaching(NotCarried::NoSession),
            ExploreError::NotYours => Self::NoSuchOrg,
            ExploreError::OutOfScope => Self::OutOfScope,
            ExploreError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for ClusterRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::OutOfScope => StatusCode::FORBIDDEN,
            Self::NoSuchOrg => StatusCode::NOT_FOUND,
            Self::Placement(_) => StatusCode::BAD_REQUEST,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the cluster view could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
