use axum::Router;
use axum::extract::{MatchedPath, Request};
use axum::http::header::AUTHORIZATION;
use axum::http::header::HeaderName;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use ferristrace_application::Stack;
use ferristrace_domain::{AgentCall, AgentToken, ApiSecret, Reader, SessionToken, TokenRefusal};

use crate::auth::bearer_token;
use crate::session_cookie;
use crate::state::AppState;

const REQUESTED_WITH: HeaderName = HeaderName::from_static("x-requested-with");
const UNROUTED: &str = "unrouted";

tokio::task_local! {
    static ROUTE: Box<str>;
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum NotCarried {
    #[error("the request carries no x-requested-with header")]
    NotFromOurOrigin,
    #[error("the request carries no live session")]
    NoSession,
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
            Self::NotFromOurOrigin => StatusCode::FORBIDDEN,
            Self::NoSession | Self::AgentsOff | Self::BadAgentToken => StatusCode::UNAUTHORIZED,
            Self::AgentIssuerUnavailable => StatusCode::SERVICE_UNAVAILABLE,
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

pub(crate) fn carried(headers: &HeaderMap) -> Result<SessionToken, NotCarried> {
    if !headers.contains_key(&REQUESTED_WITH) {
        return Err(NotCarried::NotFromOurOrigin);
    }
    session_cookie::carried(headers).ok_or(NotCarried::NoSession)
}

pub(crate) async fn reading<S: Stack>(
    state: &AppState<S>,
    headers: &HeaderMap,
) -> Result<Reader, NotCarried> {
    let offered = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(bearer_token);
    match offered {
        Some(token) if AgentToken::looks_like_one(token) => {
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
        Some(secret) => Ok(Reader::Key(ApiSecret::from_plaintext(secret))),
        None => carried(headers).map(Reader::Session),
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
