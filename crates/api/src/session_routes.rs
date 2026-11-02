use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::header::{HeaderName, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::IdentityRepos;
use ferristrace_domain::{LiveSession, RollError, SessionError, SessionId};
use serde::Serialize;
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::session_cookie;
use crate::state::AppState;

const REQUESTED_WITH: HeaderName = HeaderName::from_static("x-requested-with");

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

pub fn session_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: IdentityRepos,
{
    Router::new()
        .route("/auth/sessions", get(live::<S>).delete(end_others::<S>))
        .route("/auth/sessions/{id}", delete(end_one::<S>))
        .route("/auth/sign-out", post(sign_out::<S>))
        .with_state(state)
}

async fn sign_out<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Response, SessionsRefused>
where
    S::Repos: IdentityRepos,
{
    if !headers.contains_key(&REQUESTED_WITH) {
        return Err(SessionsRefused::NotFromOurOrigin);
    }
    let revoked = match session_cookie::carried(&headers) {
        Some(token) => state.app.sign_out(&token).await,
        None => Ok(()),
    };
    let Some(cookie) = session_cookie::cleared(state.cookie) else {
        return Err(SessionsRefused::Refused(RollError::Unavailable));
    };
    if let Err(error) = revoked {
        tracing::error!(%error, "a session could not be revoked at sign out");
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            [(SET_COOKIE, cookie)],
            "the session could not be revoked",
        )
            .into_response());
    }
    Ok(([(SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response())
}

async fn live<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Response, SessionsRefused>
where
    S::Repos: IdentityRepos,
{
    let token = carried(&headers)?;
    let roll = state.app.live_sessions(&token).await?;
    Ok(Json(roll.iter().map(said).collect::<Vec<_>>()).into_response())
}

async fn end_one<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Response, SessionsRefused>
where
    S::Repos: IdentityRepos,
{
    let token = carried(&headers)?;
    state.app.end_session(&token, SessionId::new(id)).await?;
    tracing::info!("a session was signed out by its owner");
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn end_others<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Response, SessionsRefused>
where
    S::Repos: IdentityRepos,
{
    let token = carried(&headers)?;
    let ended = state.app.end_other_sessions(&token).await?;
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
    #[error("the request carries no x-requested-with header")]
    NotFromOurOrigin,
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

impl From<SessionError> for SessionsRefused {
    fn from(error: SessionError) -> Self {
        Self::Refused(RollError::from(error))
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
            Self::NotFromOurOrigin => (StatusCode::FORBIDDEN, self.to_string()),
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
