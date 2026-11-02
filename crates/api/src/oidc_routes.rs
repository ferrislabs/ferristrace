use axum::Router;
use axum::extract::{Query, State};
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::IdentityRepos;
use ferristrace_domain::{AuthError, FederatedError, FlowError};
use serde::Deserialize;

use crate::session_cookie;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct Callback {
    code: String,
    state: String,
}

#[derive(Debug, Clone)]
struct Landing {
    after_sign_in: String,
}

pub fn oidc_router<S: Stack>(state: AppState<S>, after_sign_in: impl Into<String>) -> Router
where
    S::Repos: IdentityRepos,
{
    let landing = Landing {
        after_sign_in: after_sign_in.into(),
    };
    Router::new()
        .route("/auth/oidc/start", get(start::<S>))
        .route("/auth/oidc/callback", get(callback::<S>))
        .layer(axum::Extension(landing))
        .with_state(state)
}

async fn start<S: Stack>(State(state): State<AppState<S>>) -> Result<Response, FederatedRefused>
where
    S::Repos: IdentityRepos,
{
    let url = state.app.start_federated_sign_in().await?;
    redirect(&url)
}

async fn callback<S: Stack>(
    State(state): State<AppState<S>>,
    axum::Extension(landing): axum::Extension<Landing>,
    headers: HeaderMap,
    Query(callback): Query<Callback>,
) -> Result<Response, FederatedRefused>
where
    S::Repos: IdentityRepos,
{
    let token = state
        .app
        .finish_federated_sign_in(
            callback.code,
            &callback.state,
            session_cookie::agent(&headers).as_ref(),
        )
        .await?;
    let Some(cookie) = session_cookie::opened(&token, state.cookie) else {
        return Err(FederatedRefused::Unavailable);
    };
    let Ok(location) = axum::http::HeaderValue::from_str(&landing.after_sign_in) else {
        return Err(FederatedRefused::Unavailable);
    };
    Ok((
        StatusCode::SEE_OTHER,
        [(SET_COOKIE, cookie), (LOCATION, location)],
    )
        .into_response())
}

fn redirect(url: &str) -> Result<Response, FederatedRefused> {
    let Ok(location) = axum::http::HeaderValue::from_str(url) else {
        return Err(FederatedRefused::Unavailable);
    };
    Ok((StatusCode::SEE_OTHER, [(LOCATION, location)]).into_response())
}

#[derive(Debug, thiserror::Error)]
pub enum FederatedRefused {
    #[error("this attempt is not one we started")]
    Unknown,
    #[error("the provider did not vouch for anybody")]
    Refused,
    #[error("the sign in could not be completed")]
    Unavailable,
}

impl From<FederatedError> for FederatedRefused {
    fn from(error: FederatedError) -> Self {
        match error {
            FederatedError::Flow(FlowError::Unknown) => Self::Unknown,
            FederatedError::Auth(AuthError::Rejected | AuthError::SourceDisabled) => Self::Refused,
            FederatedError::Auth(AuthError::ProviderUnreachable)
            | FederatedError::Flow(FlowError::StoreUnreachable)
            | FederatedError::Session(_) => Self::Unavailable,
        }
    }
}

impl IntoResponse for FederatedRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Unknown => StatusCode::BAD_REQUEST,
            Self::Refused => StatusCode::UNAUTHORIZED,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a federated sign in could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
