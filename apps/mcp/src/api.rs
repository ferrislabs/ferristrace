use std::time::Duration;

use reqwest::{Method, Url};
use serde_json::Value;

use crate::fault::Fault;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_MESSAGE_CHARS: usize = 500;

#[derive(Debug, thiserror::Error)]
#[error("the API URL is not a usable base URL: {0}")]
pub struct BadBase(String);

#[derive(Debug, Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    base: Url,
}

pub fn refusal_message(body: &str) -> String {
    let trimmed = body.trim();
    let from_json = serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|value| {
            ["message", "error"]
                .iter()
                .find_map(|key| value.get(key).and_then(Value::as_str).map(str::to_owned))
        });
    let message = from_json.unwrap_or_else(|| trimmed.to_owned());
    if message.is_empty() {
        return "no detail given".to_owned();
    }
    message.chars().take(MAX_MESSAGE_CHARS).collect()
}

impl ApiClient {
    pub fn new(base: &str) -> Result<Self, BadBase> {
        let url = Url::parse(base).map_err(|error| BadBase(error.to_string()))?;
        if url.cannot_be_a_base() || !matches!(url.scheme(), "http" | "https") {
            return Err(BadBase(base.to_owned()));
        }
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|error| BadBase(error.to_string()))?;
        Ok(Self { http, base: url })
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        url
    }

    pub async fn get(
        &self,
        token: &str,
        segments: &[&str],
        query: &[(&str, String)],
    ) -> Result<Value, Fault> {
        self.send(Method::GET, token, segments, query, None).await
    }

    pub async fn post(
        &self,
        token: &str,
        segments: &[&str],
        body: Option<&Value>,
    ) -> Result<Value, Fault> {
        self.send(Method::POST, token, segments, &[], body).await
    }

    pub async fn patch(
        &self,
        token: &str,
        segments: &[&str],
        body: &Value,
    ) -> Result<Value, Fault> {
        self.send(Method::PATCH, token, segments, &[], Some(body))
            .await
    }

    async fn send(
        &self,
        method: Method,
        token: &str,
        segments: &[&str],
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Value, Fault> {
        let mut request = self
            .http
            .request(method, self.url(segments))
            .bearer_auth(token)
            .query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        let mut response = request.send().await.map_err(|error| unreachable(&error))?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| unreachable(&error))?
        {
            if bytes.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(Fault::TooLarge {
                    limit: MAX_BODY_BYTES,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(Fault::Refused {
                status: status.as_u16(),
                message: refusal_message(&String::from_utf8_lossy(&bytes)),
            });
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| Fault::Unreadable)
    }
}

fn unreachable(error: &reqwest::Error) -> Fault {
    tracing::warn!(%error, "the FerrisTrace API call failed");
    Fault::Unreachable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_text_body_is_the_message() {
        assert_eq!(
            refusal_message("this agent holds no scope that allows this\n"),
            "this agent holds no scope that allows this"
        );
    }

    #[test]
    fn a_json_body_gives_its_message_or_error_field() {
        assert_eq!(
            refusal_message(r#"{"message":"nope","status":403}"#),
            "nope"
        );
        assert_eq!(refusal_message(r#"{"error":"bad"}"#), "bad");
    }

    #[test]
    fn an_empty_body_says_so() {
        assert_eq!(refusal_message("  "), "no detail given");
    }

    #[test]
    fn a_long_message_is_cut() {
        let long = "x".repeat(MAX_MESSAGE_CHARS * 2);
        assert_eq!(refusal_message(&long).chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn segments_are_percent_encoded_under_the_base_path() {
        let api = ApiClient::new("http://api.test:3333/prefix/").unwrap();
        assert_eq!(
            api.url(&["api", "orgs", "o", "metrics", "a/b c", "series"])
                .as_str(),
            "http://api.test:3333/prefix/api/orgs/o/metrics/a%2Fb%20c/series"
        );
    }

    #[test]
    fn a_base_must_be_http() {
        assert!(ApiClient::new("ftp://x").is_err());
        assert!(ApiClient::new("not a url").is_err());
        assert!(ApiClient::new("http://localhost:3333").is_ok());
    }
}
