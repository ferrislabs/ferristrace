use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_domain::{
    AccountDirectory, AuthError, AuthProvider, Authorizer, FederatedError, FederatedSignIn,
    FlowError, OidcFlows, SessionStore,
};
use serde::Deserialize;

use crate::session_cookie::{self, SessionCookie};

#[derive(Debug, Deserialize)]
pub struct Callback {
    code: String,
    state: String,
}

#[derive(Debug, Clone)]
struct Landing {
    cookie: SessionCookie,
    after_sign_in: String,
}

pub fn oidc_router<Z, W, A, S>(
    federated: Arc<FederatedSignIn<Z, W, A, S>>,
    cookie: SessionCookie,
    after_sign_in: impl Into<String>,
) -> Router
where
    Z: Authorizer + AuthProvider + Send + Sync + 'static,
    W: OidcFlows + Send + Sync + 'static,
    A: AccountDirectory + Send + Sync + 'static,
    S: SessionStore + Send + Sync + 'static,
{
    let landing = Landing {
        cookie,
        after_sign_in: after_sign_in.into(),
    };
    Router::new()
        .route("/auth/oidc/start", get(start::<Z, W, A, S>))
        .route("/auth/oidc/callback", get(callback::<Z, W, A, S>))
        .layer(axum::Extension(landing))
        .with_state(federated)
}

async fn start<Z, W, A, S>(
    State(federated): State<Arc<FederatedSignIn<Z, W, A, S>>>,
) -> Result<Response, FederatedRefused>
where
    Z: Authorizer + AuthProvider + Send + Sync + 'static,
    W: OidcFlows + Send + Sync + 'static,
    A: AccountDirectory + Send + Sync + 'static,
    S: SessionStore + Send + Sync + 'static,
{
    let url = federated.start().await?;
    redirect(&url)
}

async fn callback<Z, W, A, S>(
    State(federated): State<Arc<FederatedSignIn<Z, W, A, S>>>,
    axum::Extension(landing): axum::Extension<Landing>,
    headers: HeaderMap,
    Query(callback): Query<Callback>,
) -> Result<Response, FederatedRefused>
where
    Z: Authorizer + AuthProvider + Send + Sync + 'static,
    W: OidcFlows + Send + Sync + 'static,
    A: AccountDirectory + Send + Sync + 'static,
    S: SessionStore + Send + Sync + 'static,
{
    let token = federated
        .finish(
            callback.code,
            &callback.state,
            session_cookie::agent(&headers).as_ref(),
        )
        .await?;
    let Some(cookie) = session_cookie::opened(&token, landing.cookie) else {
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
