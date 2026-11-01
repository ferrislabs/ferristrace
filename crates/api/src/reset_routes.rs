use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::header::HeaderName;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use ferristrace_domain::{
    Email, Mailer, Password, PasswordHasher, ResetError, ResetPassword, ResetStore, ResetToken,
    WeakPassword,
};
use serde::Deserialize;

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

pub fn reset_router<R, H, M>(reset: Arc<ResetPassword<R, H, M>>) -> Router
where
    R: ResetStore + Send + Sync + 'static,
    H: PasswordHasher + Send + Sync + 'static,
    M: Mailer + Send + Sync + 'static,
{
    Router::new()
        .route("/auth/reset/request", post(request::<R, H, M>))
        .route("/auth/reset/confirm", post(confirm::<R, H, M>))
        .with_state(reset)
}

async fn request<R, H, M>(
    State(reset): State<Arc<ResetPassword<R, H, M>>>,
    headers: HeaderMap,
    Json(asked): Json<Requested>,
) -> Result<Response, ResetRefused>
where
    R: ResetStore + Send + Sync + 'static,
    H: PasswordHasher + Send + Sync + 'static,
    M: Mailer + Send + Sync + 'static,
{
    requested_by_a_script(&headers)?;
    if let Ok(email) = Email::try_from(asked.email.as_str()) {
        tokio::spawn(async move {
            match reset.request(&email, ResetToken::random()).await {
                Ok(true) => tracing::info!("a password reset was sent"),
                Ok(false) => tracing::info!("a password reset was asked for an unknown address"),
                Err(error) => tracing::error!(%error, "a password reset was not sent"),
            }
        });
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn confirm<R, H, M>(
    State(reset): State<Arc<ResetPassword<R, H, M>>>,
    headers: HeaderMap,
    Json(chosen): Json<Chosen>,
) -> Result<Response, ResetRefused>
where
    R: ResetStore + Send + Sync + 'static,
    H: PasswordHasher + Send + Sync + 'static,
    M: Mailer + Send + Sync + 'static,
{
    requested_by_a_script(&headers)?;
    let token = ResetToken::from_plaintext(&chosen.token);
    if !token.looks_like_one() {
        return Err(ResetRefused::Link);
    }
    let ended = reset
        .confirm(&token, &Password::new(&chosen.password))
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
