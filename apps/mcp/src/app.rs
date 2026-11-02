use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Request},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::ops::Backend;
use crate::tools::FerristraceTools;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MCP_PATH: &str = "/orgs/{org}/mcp";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Organisation(pub Uuid);

pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    let (scheme, token) = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer")
        && !token.is_empty()
        && !token.contains(char::is_whitespace))
    .then_some(token)
}

pub fn router(api: ApiClient) -> Router {
    let backend = Arc::new(Backend::new(api));
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .with_max_request_body_bytes(MAX_REQUEST_BYTES)
        .disable_allowed_hosts();
    let service = StreamableHttpService::new(
        move || Ok(FerristraceTools::new(Arc::clone(&backend))),
        Arc::new(NeverSessionManager::default()),
        config,
    );
    Router::new()
        .nest_service(MCP_PATH, service)
        .layer(middleware::from_fn(admit))
        .route("/healthz", get(|| async { StatusCode::OK }))
}

async fn admit(Path(org): Path<String>, mut request: Request, next: Next) -> Response {
    if bearer(request.headers()).is_none() {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return response;
    }
    let Ok(org) = Uuid::parse_str(&org) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    request.extensions_mut().insert(Organisation(org));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn a_bearer_is_read_case_insensitively() {
        assert_eq!(bearer(&headers("Bearer abc.def")), Some("abc.def"));
        assert_eq!(bearer(&headers("bearer abc")), Some("abc"));
    }

    #[test]
    fn anything_else_is_not_a_bearer() {
        assert_eq!(bearer(&HeaderMap::new()), None);
        assert_eq!(bearer(&headers("Basic abc")), None);
        assert_eq!(bearer(&headers("Bearer ")), None);
        assert_eq!(bearer(&headers("Bearer a b")), None);
        assert_eq!(bearer(&headers("Bearer")), None);
    }
}
