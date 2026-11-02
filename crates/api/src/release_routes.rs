use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::ReadRepos;
use ferristrace_domain::{
    Counted, Deploy, ProjectId, Release, ReleaseDetail, ReleaseError, ReleaseFilter, TenantId,
    Traffic, Window,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, reading};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

pub fn release_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: ReadRepos,
{
    Router::new()
        .route("/api/orgs/{org}/releases", get(releases::<S>))
        .route(
            "/api/orgs/{org}/releases/{service}/{version}",
            get(release::<S>),
        )
        .route("/api/orgs/{org}/deploys", get(deploys::<S>))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct Asked {
    from: u64,
    to: u64,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    project: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct Narrowed {
    #[serde(default)]
    project: Option<Uuid>,
}

impl Asked {
    fn filter(self) -> Result<ReleaseFilter, ReleaseRefused> {
        let window = Window::new(
            self.from.saturating_mul(NANOS_PER_MILLI),
            self.to.saturating_mul(NANOS_PER_MILLI),
        )
        .map_err(|_| ReleaseRefused::Window)?;
        Ok(ReleaseFilter {
            window,
            service: self
                .service
                .map(|service| service.trim().to_owned())
                .filter(|service| !service.is_empty())
                .map(Into::into),
            project: self.project.map(ProjectId::new),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct Listed {
    releases: Vec<Shown>,
}

#[derive(Debug, Serialize)]
struct Shown {
    service: String,
    version: String,
    environments: Vec<String>,
    first_seen_at_ms: u64,
    last_seen_at_ms: u64,
    requests: u64,
    errors: u64,
    new_issues: u64,
}

impl From<Release> for Shown {
    fn from(release: Release) -> Self {
        Self {
            service: release.service.into_string(),
            version: release.version.into_string(),
            environments: release
                .environments
                .into_iter()
                .map(str::into_string)
                .collect(),
            first_seen_at_ms: release.first_seen_unix_nanos / NANOS_PER_MILLI,
            last_seen_at_ms: release.last_seen_unix_nanos / NANOS_PER_MILLI,
            requests: release.requests,
            errors: release.errors,
            new_issues: release.new_issues,
        }
    }
}

#[derive(Debug, Serialize)]
struct Around {
    requests: u64,
    errors: u64,
    error_rate: Option<f64>,
}

impl From<Traffic> for Around {
    fn from(traffic: Traffic) -> Self {
        Self {
            requests: traffic.requests,
            errors: traffic.errors,
            error_rate: traffic.error_rate(),
        }
    }
}

#[derive(Debug, Serialize)]
struct Introduced {
    fingerprint: String,
    project: String,
    exception_type: String,
    message: String,
    service: String,
    events: u64,
    first_seen_at_ms: u64,
    last_seen_at_ms: u64,
}

impl From<Counted> for Introduced {
    fn from(issue: Counted) -> Self {
        Self {
            fingerprint: issue.print.to_string(),
            project: issue.project.to_string(),
            exception_type: issue.exception_type.into(),
            message: issue.message.into(),
            service: issue.service.into(),
            events: issue.count,
            first_seen_at_ms: issue.first_seen_unix_nanos / NANOS_PER_MILLI,
            last_seen_at_ms: issue.last_seen_unix_nanos / NANOS_PER_MILLI,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Detailed {
    #[serde(flatten)]
    release: Shown,
    before: Around,
    after: Around,
    issues: Vec<Introduced>,
}

impl From<ReleaseDetail> for Detailed {
    fn from(detail: ReleaseDetail) -> Self {
        Self {
            release: detail.release.into(),
            before: detail.before.into(),
            after: detail.after.into(),
            issues: detail.introduced.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Marked {
    deploys: Vec<Mark>,
}

#[derive(Debug, Serialize)]
struct Mark {
    service: String,
    version: String,
    environment: String,
    at_ms: u64,
}

impl From<Deploy> for Mark {
    fn from(deploy: Deploy) -> Self {
        Self {
            at_ms: deploy.at_ms(),
            service: deploy.service.into_string(),
            version: deploy.version.into_string(),
            environment: deploy.environment.into_string(),
        }
    }
}

async fn releases<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<Asked>,
    headers: HeaderMap,
) -> Result<Json<Listed>, ReleaseRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&headers)?;
    let filter = asked.filter()?;
    let found = state
        .app
        .releases(&who, TenantId::new(org), &filter)
        .await?;
    Ok(Json(Listed {
        releases: found.into_iter().map(Into::into).collect(),
    }))
}

async fn release<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, service, version)): Path<(Uuid, String, String)>,
    Query(narrowed): Query<Narrowed>,
    headers: HeaderMap,
) -> Result<Json<Detailed>, ReleaseRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&headers)?;
    let detail = state
        .app
        .release(
            &who,
            TenantId::new(org),
            &service,
            &version,
            narrowed.project.map(ProjectId::new),
        )
        .await?;
    Ok(Json(detail.into()))
}

async fn deploys<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<Asked>,
    headers: HeaderMap,
) -> Result<Json<Marked>, ReleaseRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&headers)?;
    let filter = asked.filter()?;
    let found = state.app.deploys(&who, TenantId::new(org), &filter).await?;
    Ok(Json(Marked {
        deploys: found.into_iter().map(Into::into).collect(),
    }))
}

impl From<NotCarried> for ReleaseRefused {
    fn from(error: NotCarried) -> Self {
        Self::Reaching(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReleaseRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("a window must end after it starts")]
    Window,
    #[error("no such release")]
    NoSuchRelease,
    #[error("the releases could not be read")]
    Unavailable,
}

impl From<ReleaseError> for ReleaseRefused {
    fn from(error: ReleaseError) -> Self {
        match error {
            ReleaseError::NoSession => Self::Reaching(NotCarried::NoSession),
            ReleaseError::NotYours => Self::NoSuchOrg,
            ReleaseError::Unknown => Self::NoSuchRelease,
            ReleaseError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for ReleaseRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::NoSuchOrg | Self::NoSuchRelease => StatusCode::NOT_FOUND,
            Self::Window => StatusCode::BAD_REQUEST,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the releases could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
