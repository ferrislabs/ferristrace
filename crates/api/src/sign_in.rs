use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::header::{HeaderName, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use ferristrace_application::Stack;
use ferristrace_application::registry::IdentityRepos;
use ferristrace_domain::{
    AuthError, Email, FactorError, Opened, Password, SessionError, SignInError, SignInProof,
};
use serde::{Deserialize, Serialize};

use crate::session_cookie;
use crate::state::AppState;

const REQUESTED_WITH: HeaderName = HeaderName::from_static("x-requested-with");

#[derive(Debug, Deserialize)]
pub struct Credentials {
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct SecondStepAsked {
    second_step: Vec<&'static str>,
}

pub fn auth_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: IdentityRepos,
{
    Router::new()
        .route("/auth/sign-in", post(begin::<S>))
        .route("/auth/sign-out", post(end::<S>))
        .with_state(state)
}

async fn begin<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(credentials): Json<Credentials>,
) -> Result<Response, SignInRefused>
where
    S::Repos: IdentityRepos,
{
    requested_by_a_script(&headers)?;
    let email =
        Email::try_from(credentials.email.as_str()).map_err(|_| SignInRefused::Credentials)?;
    let proof = SignInProof::Password {
        email,
        password: Password::new(&credentials.password),
    };

    match state
        .app
        .sign_in(proof, session_cookie::agent(&headers).as_ref())
        .await?
    {
        Opened::Session(token) => {
            let Some(cookie) = session_cookie::opened(&token, state.cookie) else {
                return Err(SignInRefused::Unavailable);
            };
            Ok(([(SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response())
        }
        Opened::SecondStep { challenge, offered } => {
            let Some(cookie) = session_cookie::challenging(&challenge, state.cookie) else {
                return Err(SignInRefused::Unavailable);
            };
            let asked = SecondStepAsked {
                second_step: offered.iter().map(|kind| kind.as_str()).collect(),
            };
            Ok((StatusCode::OK, [(SET_COOKIE, cookie)], Json(asked)).into_response())
        }
    }
}

async fn end<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Response, SignInRefused>
where
    S::Repos: IdentityRepos,
{
    requested_by_a_script(&headers)?;
    if let Some(token) = session_cookie::carried(&headers) {
        state.app.sign_out(&token).await?;
    }
    let Some(cookie) = session_cookie::cleared(state.cookie) else {
        return Err(SignInRefused::Unavailable);
    };
    Ok(([(SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response())
}

fn requested_by_a_script(headers: &HeaderMap) -> Result<(), SignInRefused> {
    if headers.contains_key(&REQUESTED_WITH) {
        Ok(())
    } else {
        Err(SignInRefused::NotFromOurOrigin)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignInRefused {
    #[error("the request carries no x-requested-with header")]
    NotFromOurOrigin,
    #[error("the credentials match no account")]
    Credentials,
    #[error("this way of signing in is not enabled")]
    NotEnabled,
    #[error("the sign in could not be completed")]
    Unavailable,
}

impl From<SignInError> for SignInRefused {
    fn from(error: SignInError) -> Self {
        match error {
            SignInError::Auth(AuthError::Rejected)
            | SignInError::Session(SessionError::Unknown)
            | SignInError::Factor(FactorError::NoSession) => Self::Credentials,
            SignInError::Auth(AuthError::SourceDisabled)
            | SignInError::Factor(FactorError::NotOffered) => Self::NotEnabled,
            SignInError::Auth(AuthError::ProviderUnreachable)
            | SignInError::Session(SessionError::StoreUnreachable)
            | SignInError::Factor(_) => Self::Unavailable,
        }
    }
}

impl IntoResponse for SignInRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NotFromOurOrigin => StatusCode::FORBIDDEN,
            Self::Credentials => StatusCode::UNAUTHORIZED,
            Self::NotEnabled => StatusCode::NOT_IMPLEMENTED,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a sign in could not be served");
        }
        (status, self.to_string()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::FactorKind;

    use super::SecondStepAsked;

    #[test]
    fn the_offered_kinds_are_named_the_way_the_screen_names_them() {
        let asked = SecondStepAsked {
            second_step: vec![FactorKind::Authenticator.as_str()],
        };

        assert_eq!(
            serde_json::to_string(&asked).expect("json"),
            r#"{"second_step":["app"]}"#
        );
    }
}
