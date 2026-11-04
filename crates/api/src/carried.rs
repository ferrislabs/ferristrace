use axum::Router;
use axum::extract::{MatchedPath, Request};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use ferristrace_application::Stack;
use ferristrace_domain::{
    AgentCall, AgentToken, ApiSecret, Reader, SessionToken, TokenRefusal, UserRefusal,
};

use crate::auth::bearer_token;
use crate::state::AppState;

const UNROUTED: &str = "unrouted";

tokio::task_local! {
    static ROUTE: Box<str>;
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum NotCarried {
    #[error("the request carries no bearer token")]
    NoSession,
    #[error("signing in is not enabled on this server")]
    SignInOff,
    #[error("the token is not valid")]
    BadToken,
    #[error("the token does not belong to a person")]
    NotAPerson,
    #[error("the identity provider could not be reached")]
    IssuerUnavailable,
    #[error("agents are not enabled on this server")]
    AgentsOff,
    #[error("the agent token is not valid")]
    BadAgentToken,
    #[error("the agent issuer could not be reached")]
    AgentIssuerUnavailable,
}

impl IntoResponse for NotCarried {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NoSession
            | Self::SignInOff
            | Self::BadToken
            | Self::NotAPerson
            | Self::AgentsOff
            | Self::BadAgentToken => StatusCode::UNAUTHORIZED,
            Self::IssuerUnavailable | Self::AgentIssuerUnavailable => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        };
        (status, self.to_string()).into_response()
    }
}

impl From<TokenRefusal> for NotCarried {
    fn from(refusal: TokenRefusal) -> Self {
        match refusal {
            TokenRefusal::Off => Self::AgentsOff,
            TokenRefusal::Invalid => Self::BadAgentToken,
            TokenRefusal::Unavailable => Self::AgentIssuerUnavailable,
        }
    }
}

impl From<UserRefusal> for NotCarried {
    fn from(refusal: UserRefusal) -> Self {
        match refusal {
            UserRefusal::Off => Self::SignInOff,
            UserRefusal::NotAPerson => Self::NotAPerson,
            UserRefusal::Invalid => Self::BadToken,
            UserRefusal::Unavailable => Self::IssuerUnavailable,
        }
    }
}

fn offered(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(bearer_token)
}

pub(crate) async fn carried<S: Stack>(
    state: &AppState<S>,
    headers: &HeaderMap,
) -> Result<SessionToken, NotCarried> {
    let token = offered(headers)
        .filter(|token| AgentToken::looks_like_one(token))
        .ok_or(NotCarried::NoSession)?;
    Ok(state.app.verify_person(token).await?)
}

pub(crate) async fn reading<S: Stack>(
    state: &AppState<S>,
    headers: &HeaderMap,
) -> Result<Reader, NotCarried> {
    match offered(headers) {
        Some(token) if AgentToken::looks_like_one(token) => {
            match state.app.verify_person(token).await {
                Ok(person) => Ok(Reader::Session(person)),
                Err(UserRefusal::NotAPerson | UserRefusal::Off) => {
                    let verified = state
                        .app
                        .verify_agent(&AgentToken::from_plaintext(token))
                        .await?;
                    Ok(Reader::Agent(AgentCall {
                        token: verified,
                        route: ROUTE
                            .try_with(Clone::clone)
                            .unwrap_or_else(|_| UNROUTED.into()),
                    }))
                }
                Err(refusal) => Err(refusal.into()),
            }
        }
        Some(secret) => Ok(Reader::Key(ApiSecret::from_plaintext(secret))),
        None => Err(NotCarried::NoSession),
    }
}

async fn mark_route(request: Request, next: Next) -> Response {
    let route: Box<str> = request
        .extensions()
        .get::<MatchedPath>()
        .map_or(UNROUTED, MatchedPath::as_str)
        .into();
    ROUTE.scope(route, next.run(request)).await
}

pub fn mark_routes(router: Router) -> Router {
    router.route_layer(from_fn(mark_route))
}
