use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use ferristrace_domain::{LiveSession, RollError, SessionId, SessionRoll, SessionStore, Sessions};
use serde::Serialize;
use uuid::Uuid;

use crate::explore_routes::{NotCarried, carried};

const MILLIS_PER_SECOND: u64 = 1_000;

#[derive(Debug, Serialize)]
struct SessionSaid {
    id: String,
    from: Option<String>,
    created_at_ms: u64,
    last_seen_at_ms: u64,
    current: bool,
}

#[derive(Debug, Serialize)]
struct Ended {
    ended: u64,
}

type Rolling<S, R> = Arc<Sessions<S, R>>;

pub fn session_router<S, R>(sessions: Rolling<S, R>) -> Router
where
    S: SessionStore + Send + Sync + 'static,
    R: SessionRoll + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/auth/sessions",
            get(live::<S, R>).delete(end_others::<S, R>),
        )
        .route("/auth/sessions/{id}", delete(end_one::<S, R>))
        .with_state(sessions)
}

async fn live<S, R>(
    State(sessions): State<Rolling<S, R>>,
    headers: HeaderMap,
) -> Result<Response, SessionsRefused>
where
    S: SessionStore + Send + Sync + 'static,
    R: SessionRoll + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    let roll = sessions.live(&token).await?;
    Ok(Json(roll.iter().map(said).collect::<Vec<_>>()).into_response())
}

async fn end_one<S, R>(
    State(sessions): State<Rolling<S, R>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Response, SessionsRefused>
where
    S: SessionStore + Send + Sync + 'static,
    R: SessionRoll + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    sessions.end_one(&token, SessionId::new(id)).await?;
    tracing::info!("a session was signed out by its owner");
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn end_others<S, R>(
    State(sessions): State<Rolling<S, R>>,
    headers: HeaderMap,
) -> Result<Response, SessionsRefused>
where
    S: SessionStore + Send + Sync + 'static,
    R: SessionRoll + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    let ended = sessions.end_others(&token).await?;
    tracing::info!(sessions = ended, "every other session was signed out");
    Ok(Json(Ended { ended }).into_response())
}

fn said(session: &LiveSession) -> SessionSaid {
    SessionSaid {
        id: session.id.to_string(),
        from: session.from.as_ref().map(|agent| agent.as_str().to_owned()),
        created_at_ms: session.created_at.get().saturating_mul(MILLIS_PER_SECOND),
        last_seen_at_ms: session.last_seen_at.get().saturating_mul(MILLIS_PER_SECOND),
        current: session.current,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionsRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error(transparent)]
    Refused(RollError),
}

impl From<NotCarried> for SessionsRefused {
    fn from(error: NotCarried) -> Self {
        Self::Reaching(error)
    }
}

impl From<RollError> for SessionsRefused {
    fn from(error: RollError) -> Self {
        Self::Refused(error)
    }
}

impl IntoResponse for SessionsRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Refused(error @ RollError::NoSession) => {
                (StatusCode::UNAUTHORIZED, error.to_string())
            }
            Self::Refused(error @ RollError::NoSuchSession) => {
                (StatusCode::NOT_FOUND, error.to_string())
            }
            Self::Refused(error @ RollError::Unavailable) => {
                tracing::error!(%error, "the sessions could not be served");
                (StatusCode::SERVICE_UNAVAILABLE, error.to_string())
            }
        };
        (status, message).into_response()
    }
}
