use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use ferristrace_domain::IngestKey;
use tonic::metadata::MetadataMap;

use crate::error::ApiError;

const BEARER: &str = "Bearer";
const AUTHORIZATION_KEY: &str = "authorization";

#[derive(Debug)]
pub(crate) struct Bearer(pub(crate) IngestKey);

impl<S: Send + Sync> FromRequestParts<S> for Bearer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(bearer_token)
            .map(|token| Self(IngestKey::from_plaintext(token)))
            .ok_or(ApiError::MissingBearer)
    }
}

pub(crate) fn key_of(metadata: &MetadataMap) -> Result<IngestKey, ApiError> {
    metadata
        .get(AUTHORIZATION_KEY)
        .and_then(|value| value.to_str().ok())
        .and_then(bearer_token)
        .map(IngestKey::from_plaintext)
        .ok_or(ApiError::MissingBearer)
}

fn bearer_token(header: &str) -> Option<&str> {
    let (scheme, token) = header.split_once(' ')?;
    let token = token.trim();
    let well_formed = scheme.eq_ignore_ascii_case(BEARER)
        && !token.is_empty()
        && !token.contains(char::is_whitespace);
    well_formed.then_some(token)
}

#[cfg(test)]
mod tests {
    use super::bearer_token;

    #[test]
    fn the_bearer_scheme_is_case_insensitive() {
        assert_eq!(bearer_token("Bearer abc"), Some("abc"));
        assert_eq!(bearer_token("bearer abc"), Some("abc"));
    }

    #[test]
    fn a_malformed_header_carries_no_token() {
        assert_eq!(bearer_token("Basic abc"), None);
        assert_eq!(bearer_token("Bearer"), None);
        assert_eq!(bearer_token("Bearer "), None);
        assert_eq!(bearer_token("Bearer a b"), None);
    }
}
