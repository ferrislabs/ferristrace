use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::InboxRepos;
use ferristrace_domain::{InboxError, InboxItem, InboxItemId, TenantId};
use serde::Serialize;
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct InboxItemBody {
    id: Uuid,
    kind: &'static str,
    title: String,
    body: String,
    link: Option<String>,
    created_ms: u64,
    read_ms: Option<u64>,
}

impl From<InboxItem> for InboxItemBody {
    fn from(item: InboxItem) -> Self {
        Self {
            id: item.id.as_uuid(),
            kind: item.kind.as_str(),
            title: item.title.into_string(),
            body: item.body.into_string(),
            link: item.link.map(Into::into),
            created_ms: item.created_ms,
            read_ms: item.read_ms,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Notifications {
    notifications: Vec<InboxItemBody>,
}

#[derive(Debug, Serialize)]
pub struct MarkedAllRead {
    marked: u64,
}

pub fn inbox_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: InboxRepos,
{
    Router::new()
        .route("/api/orgs/{org}/notifications", get(list::<S>))
        .route(
            "/api/orgs/{org}/notifications/read-all",
            post(mark_all_read::<S>),
        )
        .route(
            "/api/orgs/{org}/notifications/{id}/read",
            post(mark_read::<S>),
        )
        .with_state(state)
}

async fn list<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Notifications>, InboxRefused>
where
    S::Repos: InboxRepos,
{
    let token = carried(&state, &headers).await?;
    let notifications = state.app.notifications(&token, TenantId::new(org)).await?;
    Ok(Json(Notifications {
        notifications: notifications.into_iter().map(InboxItemBody::from).collect(),
    }))
}

async fn mark_read<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, InboxRefused>
where
    S::Repos: InboxRepos,
{
    let token = carried(&state, &headers).await?;
    state
        .app
        .mark_notification_read(&token, TenantId::new(org), InboxItemId::new(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_all_read<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<MarkedAllRead>, InboxRefused>
where
    S::Repos: InboxRepos,
{
    let token = carried(&state, &headers).await?;
    let marked = state
        .app
        .mark_all_notifications_read(&token, TenantId::new(org))
        .await?;
    Ok(Json(MarkedAllRead { marked }))
}

#[derive(Debug, thiserror::Error)]
pub enum InboxRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Refused(InboxError),
}

impl From<InboxError> for InboxRefused {
    fn from(error: InboxError) -> Self {
        match error {
            InboxError::NoSession => Self::Reaching(NotCarried::NoSession),
            other => Self::Refused(other),
        }
    }
}

impl IntoResponse for InboxRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Refused(InboxError::NotYours) => {
                (StatusCode::NOT_FOUND, "no such organisation".to_owned())
            }
            Self::Refused(InboxError::NoSuchItem) => {
                (StatusCode::NOT_FOUND, InboxError::NoSuchItem.to_string())
            }
            Self::Refused(error @ InboxError::StoreUnreachable) => {
                (StatusCode::SERVICE_UNAVAILABLE, error.to_string())
            }
            Self::Refused(InboxError::NoSession) => {
                (StatusCode::UNAUTHORIZED, InboxError::NoSession.to_string())
            }
        };
        if status.is_server_error() {
            tracing::error!(%message, "the inbox could not be served");
        }
        (status, message).into_response()
    }
}
