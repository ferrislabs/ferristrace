use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::secret::Secret;

const SAFETY_MARGIN: Duration = Duration::from_secs(10);
const DEFAULT_LIFETIME: Duration = Duration::from_secs(60);
const MAX_ANSWER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("the token endpoint did not answer in time")]
    Timeout,
    #[error("the token endpoint refused or could not be reached")]
    Unavailable,
    #[error("the token endpoint answered unreadably")]
    Malformed,
}

pub fn classify(error: &reqwest::Error) -> TokenError {
    if error.is_timeout() {
        TokenError::Timeout
    } else {
        TokenError::Unavailable
    }
}

#[derive(Debug, Deserialize)]
pub struct TokenAnswer {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

pub async fn token_request(
    http: &reqwest::Client,
    token_url: &str,
    timeout: Duration,
    client_id: &str,
    client_secret: &Secret,
    mut form: Vec<(&str, &str)>,
) -> Result<TokenAnswer, TokenError> {
    form.push(("client_id", client_id));
    form.push(("client_secret", client_secret.expose()));
    let mut response = http
        .post(token_url)
        .timeout(timeout)
        .form(&form)
        .send()
        .await
        .map_err(|error| {
            let kind = classify(&error);
            tracing::warn!(?kind, "the token endpoint call failed");
            kind
        })?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "the token endpoint refused the request");
        return Err(TokenError::Unavailable);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| classify(&error))? {
        if body.len() + chunk.len() > MAX_ANSWER_BYTES {
            return Err(TokenError::Malformed);
        }
        body.extend_from_slice(&chunk);
    }
    let answer: TokenAnswer = serde_json::from_slice(&body).map_err(|_| TokenError::Malformed)?;
    if answer.access_token.is_empty() {
        return Err(TokenError::Malformed);
    }
    Ok(answer)
}

pub fn lifetime_of(answer: &TokenAnswer) -> Duration {
    answer
        .expires_in
        .map_or(DEFAULT_LIFETIME, Duration::from_secs)
}

#[derive(Debug)]
pub struct ClientCredentials {
    http: reqwest::Client,
    token_url: String,
    client_id: String,
    client_secret: Secret,
    audience: Option<String>,
    timeout: Duration,
    held: Mutex<Option<(String, Instant)>>,
}

impl ClientCredentials {
    pub fn new(
        http: reqwest::Client,
        token_url: String,
        client_id: String,
        client_secret: Secret,
        audience: Option<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            http,
            token_url,
            client_id,
            client_secret,
            audience,
            timeout,
            held: Mutex::new(None),
        }
    }

    pub async fn token(&self) -> Result<String, TokenError> {
        let now = Instant::now();
        let current = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some((token, until)) = current
            && now < until
        {
            return Ok(token);
        }
        let mut form = vec![("grant_type", "client_credentials")];
        if let Some(audience) = &self.audience {
            form.push(("audience", audience));
        }
        let answer = token_request(
            &self.http,
            &self.token_url,
            self.timeout,
            &self.client_id,
            &self.client_secret,
            form,
        )
        .await?;
        let lifetime = lifetime_of(&answer).saturating_sub(SAFETY_MARGIN);
        *self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((answer.access_token.clone(), now + lifetime));
        Ok(answer.access_token)
    }

    pub fn forget(&self) {
        *self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}
