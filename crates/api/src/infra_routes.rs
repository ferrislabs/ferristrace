use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::ReadRepos;
use ferristrace_domain::{
    ExploreError, InfraFilter, PlacementName, ProjectId, Resource, ResourceKind, ResourceStatus,
    TenantId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, reading};
use crate::placement::Misplaced;
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

pub fn infra_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: ReadRepos,
{
    Router::new()
        .route("/api/orgs/{org}/infrastructure", get(infrastructure::<S>))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct Looked {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    cluster: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    project: Option<Uuid>,
}

fn given(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|held| !held.is_empty())
}

fn named(text: Option<&str>, field: &'static str) -> Result<Option<PlacementName>, Misplaced> {
    given(text)
        .map(|held| PlacementName::try_from(held).map_err(|_| Misplaced(field)))
        .transpose()
}

impl Looked {
    fn filter(&self) -> Result<InfraFilter, Misplaced> {
        Ok(InfraFilter {
            kind: given(self.kind.as_deref())
                .map(|held| ResourceKind::parse(held).ok_or(Misplaced("kind")))
                .transpose()?,
            cluster: named(self.cluster.as_deref(), "cluster")?,
            environment: named(self.environment.as_deref(), "environment")?,
            provider: named(self.provider.as_deref(), "provider")?,
            region: named(self.region.as_deref(), "region")?,
            status: given(self.status.as_deref())
                .map(|held| ResourceStatus::parse(held).ok_or(Misplaced("status")))
                .transpose()?,
            project: self.project.map(ProjectId::new),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct Shown {
    resources: Vec<ShownResource>,
}

#[derive(Debug, Serialize)]
struct ShownResource {
    kind: &'static str,
    id: String,
    name: String,
    cluster: Option<String>,
    environment: Option<String>,
    provider: Option<String>,
    region: Option<String>,
    zone: Option<String>,
    host_type: Option<String>,
    first_seen_at_ms: u64,
    last_seen_at_ms: u64,
    silent: bool,
    cpu_percent: Option<f64>,
    memory_bytes: Option<u64>,
}

fn present(held: Box<str>) -> Option<String> {
    Some(held.into_string()).filter(|text| !text.is_empty())
}

impl From<Resource> for ShownResource {
    fn from(resource: Resource) -> Self {
        Self {
            kind: resource.kind.name(),
            id: resource.id.into_string(),
            name: resource.name.into_string(),
            cluster: present(resource.cluster),
            environment: present(resource.environment),
            provider: present(resource.provider),
            region: present(resource.region),
            zone: present(resource.zone),
            host_type: present(resource.host_type),
            first_seen_at_ms: resource.first_seen_unix_nanos / NANOS_PER_MILLI,
            last_seen_at_ms: resource.last_seen_unix_nanos / NANOS_PER_MILLI,
            silent: resource.silent,
            cpu_percent: resource.cpu_percent.filter(|value| value.is_finite()),
            memory_bytes: resource.memory_bytes,
        }
    }
}

async fn infrastructure<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(looked): Query<Looked>,
    headers: HeaderMap,
) -> Result<Json<Shown>, InfraRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let filter = looked.filter()?;
    let resources = state
        .app
        .infrastructure(&who, TenantId::new(org), &filter)
        .await?;
    Ok(Json(Shown {
        resources: resources.into_iter().map(Into::into).collect(),
    }))
}

impl From<NotCarried> for InfraRefused {
    fn from(error: NotCarried) -> Self {
        Self::Reaching(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InfraRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error(transparent)]
    Placement(#[from] Misplaced),
    #[error("the infrastructure could not be read")]
    Unavailable,
}

impl From<ExploreError> for InfraRefused {
    fn from(error: ExploreError) -> Self {
        match error {
            ExploreError::NoSession => Self::Reaching(NotCarried::NoSession),
            ExploreError::NotYours => Self::NoSuchOrg,
            ExploreError::OutOfScope => Self::OutOfScope,
            ExploreError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for InfraRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::OutOfScope => StatusCode::FORBIDDEN,
            Self::NoSuchOrg => StatusCode::NOT_FOUND,
            Self::Placement(_) => StatusCode::BAD_REQUEST,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the infrastructure could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
