use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use ferristrace_application::Stack;
use ferristrace_application::registry::IdentityRepos;
use ferristrace_domain::{ChangeError, Password};
use serde::Deserialize;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct Chosen {
    current: String,
    password: String,
}

pub fn password_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: IdentityRepos,
{
    Router::new()
        .route("/auth/password", post(change_it::<S>))
        .with_state(state)
}

async fn change_it<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(chosen): Json<Chosen>,
) -> Result<Response, ChangeRefused>
where
    S::Repos: IdentityRepos,
{
    let token = carried(&headers).map_err(ChangeRefused::Reaching)?;
    let ended = state
        .app
        .change_password(
            &token,
            &Password::new(&chosen.current),
            &Password::new(&chosen.password),
        )
        .await?;
    tracing::info!(sessions = ended, "a password was changed by its owner");
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, thiserror::Error)]
pub enum ChangeRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error(transparent)]
    Refused(ChangeError),
}

impl From<ChangeError> for ChangeRefused {
    fn from(error: ChangeError) -> Self {
        Self::Refused(error)
    }
}

impl IntoResponse for ChangeRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Refused(ChangeError::NoSession) => {
                (StatusCode::UNAUTHORIZED, ChangeError::NoSession.to_string())
            }
            Self::Refused(error @ (ChangeError::NotTheCurrent | ChangeError::Weak(_))) => {
                (StatusCode::UNPROCESSABLE_ENTITY, error.to_string())
            }
            Self::Refused(error @ ChangeError::Unavailable) => {
                tracing::error!(%error, "a password change could not be served");
                (StatusCode::SERVICE_UNAVAILABLE, error.to_string())
            }
        };
        (status, message).into_response()
    }
}
