use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::header::HeaderName;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use ferristrace_application::Stack;
use ferristrace_application::registry::IdentityRepos;
use ferristrace_domain::{Email, Password, ResetError, ResetToken, WeakPassword};
use serde::Deserialize;

use crate::state::AppState;

const REQUESTED_WITH: HeaderName = HeaderName::from_static("x-requested-with");

#[derive(Debug, Deserialize)]
pub struct Requested {
    email: String,
}

#[derive(Debug, Deserialize)]
pub struct Chosen {
    token: String,
    password: String,
}

pub fn reset_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: IdentityRepos,
{
    Router::new()
        .route("/auth/reset/request", post(request::<S>))
        .route("/auth/reset/confirm", post(confirm::<S>))
        .with_state(state)
}

async fn request<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(asked): Json<Requested>,
) -> Result<Response, ResetRefused>
where
    S::Repos: IdentityRepos,
{
    requested_by_a_script(&headers)?;
    if let Ok(email) = Email::try_from(asked.email.as_str()) {
        let app = Arc::clone(&state.app);
        tokio::spawn(async move {
            match app.request_reset(&email, ResetToken::random()).await {
                Ok(true) => tracing::info!("a password reset was sent"),
                Ok(false) => tracing::info!("a password reset was asked for an unknown address"),
                Err(error) => tracing::error!(%error, "a password reset was not sent"),
            }
        });
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn confirm<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(chosen): Json<Chosen>,
) -> Result<Response, ResetRefused>
where
    S::Repos: IdentityRepos,
{
    requested_by_a_script(&headers)?;
    let token = ResetToken::from_plaintext(&chosen.token);
    if !token.looks_like_one() {
        return Err(ResetRefused::Link);
    }
    let ended = state
        .app
        .confirm_reset(&token, &Password::new(&chosen.password))
        .await?;
    tracing::info!(sessions = ended, "a password was reset");
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn requested_by_a_script(headers: &HeaderMap) -> Result<(), ResetRefused> {
    if headers.contains_key(&REQUESTED_WITH) {
        Ok(())
    } else {
        Err(ResetRefused::NotFromOurOrigin)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResetRefused {
    #[error("the request carries no x-requested-with header")]
    NotFromOurOrigin,
    #[error("this link is spent or was never issued")]
    Link,
    #[error(transparent)]
    Weak(WeakPassword),
    #[error("the password could not be changed")]
    Unavailable,
}

impl From<ResetError> for ResetRefused {
    fn from(error: ResetError) -> Self {
        match error {
            ResetError::Unknown => Self::Link,
            ResetError::Weak(weak) => Self::Weak(weak),
            ResetError::StoreUnreachable | ResetError::Undeliverable | ResetError::Hashing(_) => {
                Self::Unavailable
            }
        }
    }
}

impl IntoResponse for ResetRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NotFromOurOrigin => StatusCode::FORBIDDEN,
            Self::Link => StatusCode::BAD_REQUEST,
            Self::Weak(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a password reset could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
