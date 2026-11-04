use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, put};
use ferristrace_application::Stack;
use ferristrace_application::registry::CatalogueRepos;
use ferristrace_domain::{
    BadProjectName, CatalogueError, Project, ProjectId, ProjectName, TeamId, TenantId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Serialize)]
pub struct Held {
    projects: Vec<Kept>,
}

#[derive(Debug, Serialize)]
struct Kept {
    id: Uuid,
    name: String,
    created_at_ms: u64,
    team: Option<Owner>,
    keys: u32,
}

#[derive(Debug, Serialize)]
struct Owner {
    id: Uuid,
    name: String,
}

impl From<Project> for Kept {
    fn from(project: Project) -> Self {
        Self {
            id: project.id.as_uuid(),
            name: project.name.into_string(),
            created_at_ms: project.created_at_unix_nanos / NANOS_PER_MILLI,
            team: project.team.map(|team| Owner {
                id: team.id.as_uuid(),
                name: team.name.into_string(),
            }),
            keys: project.keys,
        }
    }
}

fn held(projects: Vec<Project>) -> Json<Held> {
    Json(Held {
        projects: projects.into_iter().map(Kept::from).collect(),
    })
}

#[derive(Debug, Deserialize)]
pub struct Naming {
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct Owning {
    team: Option<Uuid>,
}

pub fn catalogue_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: CatalogueRepos,
{
    Router::new()
        .route(
            "/api/orgs/{org}/projects",
            get(projects::<S>).post(create::<S>),
        )
        .route(
            "/api/orgs/{org}/projects/{project}",
            patch(rename::<S>).delete(forget::<S>),
        )
        .route("/api/orgs/{org}/projects/{project}/team", put(assign::<S>))
        .with_state(state)
}

async fn projects<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Held>, ProjectRefused>
where
    S::Repos: CatalogueRepos,
{
    let token = carried(&state, &headers).await?;
    Ok(held(
        state.app.list_projects(&token, TenantId::new(org)).await?,
    ))
}

async fn create<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Naming>,
) -> Result<(StatusCode, Json<Held>), ProjectRefused>
where
    S::Repos: CatalogueRepos,
{
    let token = carried(&state, &headers).await?;
    let name = ProjectName::try_from(asked.name.as_str())?;
    let kept = state
        .app
        .create_project(&token, TenantId::new(org), &name)
        .await?;
    tracing::info!(org = %org, "a project was created");
    Ok((StatusCode::CREATED, held(kept)))
}

async fn rename<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, project)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<Naming>,
) -> Result<Json<Held>, ProjectRefused>
where
    S::Repos: CatalogueRepos,
{
    let token = carried(&state, &headers).await?;
    let name = ProjectName::try_from(asked.name.as_str())?;
    let kept = state
        .app
        .rename_project(&token, TenantId::new(org), ProjectId::new(project), &name)
        .await?;
    tracing::info!(org = %org, project = %project, "a project was renamed");
    Ok(held(kept))
}

async fn forget<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, project)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<Held>, ProjectRefused>
where
    S::Repos: CatalogueRepos,
{
    let token = carried(&state, &headers).await?;
    let kept = state
        .app
        .forget_project(&token, TenantId::new(org), ProjectId::new(project))
        .await?;
    tracing::info!(org = %org, project = %project, "a project was forgotten");
    Ok(held(kept))
}

async fn assign<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, project)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<Owning>,
) -> Result<Json<Held>, ProjectRefused>
where
    S::Repos: CatalogueRepos,
{
    let token = carried(&state, &headers).await?;
    let team = asked.team.map(TeamId::new);
    let kept = state
        .app
        .assign_project(&token, TenantId::new(org), ProjectId::new(project), team)
        .await?;
    tracing::info!(org = %org, project = %project, owned = team.is_some(), "a project changed hands");
    Ok(held(kept))
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Name(#[from] BadProjectName),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("no such project")]
    NoSuchProject,
    #[error("no such team")]
    NoSuchTeam,
    #[error("the projects of an organisation are an administrator's to arrange")]
    NotAnAdministrator,
    #[error("this organisation already holds a project under that name")]
    NameTaken,
    #[error("the projects could not be read")]
    Unavailable,
}

impl From<CatalogueError> for ProjectRefused {
    fn from(error: CatalogueError) -> Self {
        match error {
            CatalogueError::NoSession => Self::Reaching(NotCarried::NoSession),
            CatalogueError::NotYours => Self::NoSuchOrg,
            CatalogueError::NotYoursToChange => Self::NotAnAdministrator,
            CatalogueError::NoSuchProject => Self::NoSuchProject,
            CatalogueError::NoSuchTeam => Self::NoSuchTeam,
            CatalogueError::NameTaken => Self::NameTaken,
            CatalogueError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for ProjectRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Name(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NoSuchOrg | Self::NoSuchProject | Self::NoSuchTeam => StatusCode::NOT_FOUND,
            Self::NotAnAdministrator => StatusCode::FORBIDDEN,
            Self::NameTaken => StatusCode::CONFLICT,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the projects could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
