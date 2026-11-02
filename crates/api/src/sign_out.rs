use axum::Router;
use axum::extract::State;
use axum::http::header::{HeaderName, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use ferristrace_application::Stack;
use ferristrace_application::registry::IdentityRepos;
use ferristrace_domain::{AuthError, SessionError, SignInError};

use crate::session_cookie;
use crate::state::AppState;

const REQUESTED_WITH: HeaderName = HeaderName::from_static("x-requested-with");

pub fn sign_out_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: IdentityRepos,
{
    Router::new()
        .route("/auth/sign-out", post(end::<S>))
        .with_state(state)
}

async fn end<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Response, SignOutRefused>
where
    S::Repos: IdentityRepos,
{
    requested_by_a_script(&headers)?;
    if let Some(token) = session_cookie::carried(&headers) {
        state.app.sign_out(&token).await?;
    }
    let Some(cookie) = session_cookie::cleared(state.cookie) else {
        return Err(SignOutRefused::Unavailable);
    };
    Ok(([(SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response())
}

fn requested_by_a_script(headers: &HeaderMap) -> Result<(), SignOutRefused> {
    if headers.contains_key(&REQUESTED_WITH) {
        Ok(())
    } else {
        Err(SignOutRefused::NotFromOurOrigin)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignOutRefused {
    #[error("the request carries no x-requested-with header")]
    NotFromOurOrigin,
    #[error("the session is not known")]
    Unknown,
    #[error("this way of signing out is not enabled")]
    NotEnabled,
    #[error("the sign out could not be completed")]
    Unavailable,
}

impl From<SignInError> for SignOutRefused {
    fn from(error: SignInError) -> Self {
        match error {
            SignInError::Auth(AuthError::Rejected)
            | SignInError::Session(SessionError::Unknown) => Self::Unknown,
            SignInError::Auth(AuthError::SourceDisabled) => Self::NotEnabled,
            SignInError::Auth(AuthError::ProviderUnreachable)
            | SignInError::Session(SessionError::StoreUnreachable) => Self::Unavailable,
        }
    }
}

impl IntoResponse for SignOutRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NotFromOurOrigin => StatusCode::FORBIDDEN,
            Self::Unknown => StatusCode::UNAUTHORIZED,
            Self::NotEnabled => StatusCode::NOT_IMPLEMENTED,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a sign out could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
