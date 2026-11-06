use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::ViewRepos;
use ferristrace_domain::{
    AccountId, BadViewDefinition, BadViewName, NewView, TenantId, View, ViewChange, ViewDefinition,
    ViewError, ViewId, ViewName, ViewSignal, Visibility,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Serialize)]
pub struct Shelf {
    views: Vec<Shown>,
}

#[derive(Debug, Serialize)]
pub struct Shown {
    id: Uuid,
    signal: &'static str,
    name: String,
    visibility: &'static str,
    definition: serde_json::Value,
    created_by: Option<Uuid>,
    created_at_ms: u64,
    updated_at_ms: u64,
}

impl From<View> for Shown {
    fn from(view: View) -> Self {
        Self {
            id: view.id.as_uuid(),
            signal: view.signal.as_str(),
            name: view.name.into_string(),
            visibility: view.visibility.as_str(),
            definition: serde_json::from_str(&view.definition).unwrap_or(serde_json::Value::Null),
            created_by: view.created_by.map(AccountId::as_uuid),
            created_at_ms: view.created_at_unix_nanos / NANOS_PER_MILLI,
            updated_at_ms: view.updated_at_unix_nanos / NANOS_PER_MILLI,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Listing {
    #[serde(default = "traces")]
    signal: String,
}

fn traces() -> String {
    ViewSignal::Traces.as_str().to_owned()
}

#[derive(Debug, Deserialize)]
pub struct Creating {
    #[serde(default = "traces")]
    signal: String,
    name: String,
    visibility: String,
    definition: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct Changing {
    name: String,
    definition: serde_json::Value,
    #[serde(default)]
    visibility: Option<String>,
}

pub fn view_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: ViewRepos,
{
    Router::new()
        .route("/api/orgs/{org}/views", get(views::<S>).post(create::<S>))
        .route(
            "/api/orgs/{org}/views/{view}",
            axum::routing::put(change::<S>).delete(remove::<S>),
        )
        .with_state(state)
}

async fn views<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(listing): Query<Listing>,
    headers: HeaderMap,
) -> Result<Json<Shelf>, ViewRefused>
where
    S::Repos: ViewRepos,
{
    let token = carried(&state, &headers).await?;
    let signal = signal_of(&listing.signal)?;
    let found = state
        .app
        .list_views(&token, TenantId::new(org), signal)
        .await?;
    Ok(Json(Shelf {
        views: found.into_iter().map(Shown::from).collect(),
    }))
}

async fn create<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Creating>,
) -> Result<(StatusCode, Json<Shown>), ViewRefused>
where
    S::Repos: ViewRepos,
{
    let token = carried(&state, &headers).await?;
    let view = NewView {
        signal: signal_of(&asked.signal)?,
        name: ViewName::try_from(asked.name.as_str())?,
        visibility: visibility_of(&asked.visibility)?,
        definition: definition_of(&asked.definition)?,
    };
    let made = state
        .app
        .create_view(&token, TenantId::new(org), &view)
        .await?;
    tracing::info!(org = %org, view = %made.id, "a view was saved");
    Ok((StatusCode::CREATED, Json(made.into())))
}

async fn change<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<Changing>,
) -> Result<Json<Shown>, ViewRefused>
where
    S::Repos: ViewRepos,
{
    let token = carried(&state, &headers).await?;
    let change = ViewChange {
        name: ViewName::try_from(asked.name.as_str())?,
        definition: definition_of(&asked.definition)?,
        visibility: asked.visibility.as_deref().map(visibility_of).transpose()?,
    };
    let saved = state
        .app
        .change_view(&token, TenantId::new(org), ViewId::new(id), &change)
        .await?;
    Ok(Json(saved.into()))
}

async fn remove<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, ViewRefused>
where
    S::Repos: ViewRepos,
{
    let token = carried(&state, &headers).await?;
    state
        .app
        .delete_view(&token, TenantId::new(org), ViewId::new(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn signal_of(spelling: &str) -> Result<ViewSignal, ViewRefused> {
    ViewSignal::try_from(spelling).map_err(|()| ViewRefused::Signal)
}

fn visibility_of(spelling: &str) -> Result<Visibility, ViewRefused> {
    Visibility::try_from(spelling).map_err(|()| ViewRefused::Visibility)
}

fn definition_of(json: &serde_json::Value) -> Result<ViewDefinition, ViewRefused> {
    Ok(ViewDefinition::try_from(json.to_string().as_str())?)
}

#[derive(Debug, thiserror::Error)]
pub enum ViewRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Name(#[from] BadViewName),
    #[error(transparent)]
    Definition(#[from] BadViewDefinition),
    #[error("views exist for traces")]
    Signal,
    #[error("a view is private or shared with the organisation")]
    Visibility,
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("no such view")]
    NoSuchView,
    #[error("only its creator, an owner or an administrator may change this view")]
    NotYoursToChange,
    #[error("only the creator of a view may change who sees it")]
    NotYoursToShare,
    #[error("a view of that name already exists")]
    NameTaken,
    #[error("the views could not be read")]
    Unavailable,
}

impl From<ViewError> for ViewRefused {
    fn from(error: ViewError) -> Self {
        match error {
            ViewError::NoSession => Self::Reaching(NotCarried::NoSession),
            ViewError::NotYours => Self::NoSuchOrg,
            ViewError::NoSuchView => Self::NoSuchView,
            ViewError::NotYoursToChange => Self::NotYoursToChange,
            ViewError::NotYoursToShare => Self::NotYoursToShare,
            ViewError::NameTaken => Self::NameTaken,
            ViewError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for ViewRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Name(_) | Self::Definition(_) | Self::Signal | Self::Visibility => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::NoSuchOrg | Self::NoSuchView => StatusCode::NOT_FOUND,
            Self::NotYoursToChange | Self::NotYoursToShare => StatusCode::FORBIDDEN,
            Self::NameTaken => StatusCode::CONFLICT,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the views could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
