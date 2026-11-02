use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{post, put};
use ferristrace_application::Stack;
use ferristrace_application::registry::{FoundRepos, SwitchRepos};
use ferristrace_domain::{BadName, FoundError, OrgName, SwitchError, TenantId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct Switching {
    org: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct Founding {
    name: String,
}

#[derive(Debug, Serialize)]
pub struct Founded {
    id: Uuid,
    name: String,
    project: Uuid,
}

pub fn workspace_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: SwitchRepos + FoundRepos,
{
    Router::new()
        .route("/auth/session/org", put(switch::<S>))
        .route("/api/orgs", post(found::<S>))
        .with_state(state)
}

async fn switch<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(asked): Json<Switching>,
) -> Result<StatusCode, WorkspaceRefused>
where
    S::Repos: SwitchRepos + FoundRepos,
{
    let token = carried(&headers)?;
    state
        .app
        .switch_org(&token, TenantId::new(asked.org))
        .await?;
    tracing::info!(org = %asked.org, "a session switched organisation");
    Ok(StatusCode::NO_CONTENT)
}

async fn found<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(asked): Json<Founding>,
) -> Result<Response, WorkspaceRefused>
where
    S::Repos: SwitchRepos + FoundRepos,
{
    let token = carried(&headers)?;
    let name = OrgName::try_from(asked.name.as_str())?;
    let founded = state.app.found_org(&token, &name).await?;
    tracing::info!(org = %founded.org, "an account founded an organisation");
    Ok((
        StatusCode::CREATED,
        Json(Founded {
            id: founded.org.as_uuid(),
            name: name.as_str().to_owned(),
            project: founded.project.as_uuid(),
        }),
    )
        .into_response())
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Name(#[from] BadName),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("the organisation could not be reached")]
    Unavailable,
}

impl From<SwitchError> for WorkspaceRefused {
    fn from(error: SwitchError) -> Self {
        match error {
            SwitchError::NoSession => Self::Reaching(NotCarried::NoSession),
            SwitchError::NotYours => Self::NoSuchOrg,
            SwitchError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl From<FoundError> for WorkspaceRefused {
    fn from(error: FoundError) -> Self {
        match error {
            FoundError::NoSession => Self::Reaching(NotCarried::NoSession),
            FoundError::Unreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for WorkspaceRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Name(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NoSuchOrg => StatusCode::NOT_FOUND,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a workspace could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
