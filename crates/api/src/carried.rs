use axum::http::header::HeaderName;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use ferristrace_domain::SessionToken;

use crate::session_cookie;

const REQUESTED_WITH: HeaderName = HeaderName::from_static("x-requested-with");

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum NotCarried {
    #[error("the request carries no x-requested-with header")]
    NotFromOurOrigin,
    #[error("the request carries no live session")]
    NoSession,
}

impl IntoResponse for NotCarried {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NotFromOurOrigin => StatusCode::FORBIDDEN,
            Self::NoSession => StatusCode::UNAUTHORIZED,
        };
        (status, self.to_string()).into_response()
    }
}

pub(crate) fn carried(headers: &HeaderMap) -> Result<SessionToken, NotCarried> {
    if !headers.contains_key(&REQUESTED_WITH) {
        return Err(NotCarried::NotFromOurOrigin);
    }
    session_cookie::carried(headers).ok_or(NotCarried::NoSession)
}
