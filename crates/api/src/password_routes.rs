use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use ferristrace_domain::{
    AuthProvider, ChangeError, ChangePassword, Password, PasswordHasher, ResetStore, SessionStore,
};
use serde::Deserialize;

use crate::carried::{NotCarried, carried};

#[derive(Debug, Deserialize)]
pub struct Chosen {
    current: String,
    password: String,
}

pub fn password_router<S, L, R, H>(change: Arc<ChangePassword<S, L, R, H>>) -> Router
where
    S: SessionStore + Send + Sync + 'static,
    L: AuthProvider + Send + Sync + 'static,
    R: ResetStore + Send + Sync + 'static,
    H: PasswordHasher + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/password", post(change_it::<S, L, R, H>))
        .with_state(change)
}

async fn change_it<S, L, R, H>(
    State(change): State<Arc<ChangePassword<S, L, R, H>>>,
    headers: HeaderMap,
    Json(chosen): Json<Chosen>,
) -> Result<Response, ChangeRefused>
where
    S: SessionStore + Send + Sync + 'static,
    L: AuthProvider + Send + Sync + 'static,
    R: ResetStore + Send + Sync + 'static,
    H: PasswordHasher + Send + Sync + 'static,
{
    let token = carried(&headers).map_err(ChangeRefused::Reaching)?;
    let ended = change
        .change(
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
