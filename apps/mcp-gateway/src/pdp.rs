use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::credentials::ClientCredentials;
use crate::reason::Reason;

const MAX_BATCH: usize = 100;
const MAX_ANSWER_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PdpError {
    #[error("the policy decision point timed out")]
    Timeout,
    #[error("the policy decision point is unavailable")]
    Unavailable,
    #[error("the policy decision point answered unreadably")]
    Malformed,
}

impl PdpError {
    pub fn reason(self) -> Reason {
        match self {
            Self::Timeout => Reason::PdpTimeout,
            Self::Unavailable => Reason::PdpUnavailable,
            Self::Malformed => Reason::PdpMalformed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub allowed: bool,
    pub policies: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WireDecision {
    decision: bool,
    #[serde(default)]
    context: Option<WireContext>,
}

#[derive(Debug, Deserialize)]
struct WireContext {
    #[serde(default)]
    reason_admin: Option<WireReason>,
}

#[derive(Debug, Deserialize)]
struct WireReason {
    #[serde(default)]
    policies: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WireBatch {
    evaluations: Vec<WireDecision>,
}

impl From<WireDecision> for Verdict {
    fn from(wire: WireDecision) -> Self {
        Self {
            allowed: wire.decision,
            policies: wire
                .context
                .and_then(|context| context.reason_admin)
                .map(|reason| reason.policies)
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug)]
pub struct Pdp {
    http: reqwest::Client,
    single_url: String,
    batch_url: String,
    credentials: ClientCredentials,
    timeout: Duration,
}

impl Pdp {
    pub fn new(
        http: reqwest::Client,
        single_url: String,
        batch_url: String,
        credentials: ClientCredentials,
        timeout: Duration,
    ) -> Self {
        Self {
            http,
            single_url,
            batch_url,
            credentials,
            timeout,
        }
    }

    pub async fn evaluate(
        &self,
        subject: &Value,
        action: &str,
        resource: &Value,
        context: &Value,
    ) -> Result<Verdict, PdpError> {
        let body = json!({
            "subject": subject,
            "action": { "name": action },
            "resource": resource,
            "context": context,
        });
        let wire: WireDecision = self.post(&self.single_url, &body).await?;
        Ok(wire.into())
    }

    pub async fn evaluate_many(
        &self,
        subject: &Value,
        action: &str,
        resources: &[Value],
        context: &Value,
    ) -> Result<Vec<Verdict>, PdpError> {
        let mut verdicts = Vec::with_capacity(resources.len());
        for chunk in resources.chunks(MAX_BATCH) {
            let body = json!({
                "subject": subject,
                "action": { "name": action },
                "context": context,
                "evaluations": chunk
                    .iter()
                    .map(|resource| json!({ "resource": resource }))
                    .collect::<Vec<_>>(),
                "options": { "evaluations_semantic": "execute_all" },
            });
            let batch: WireBatch = self.post(&self.batch_url, &body).await?;
            if batch.evaluations.len() != chunk.len() {
                return Err(PdpError::Malformed);
            }
            verdicts.extend(batch.evaluations.into_iter().map(Verdict::from));
        }
        Ok(verdicts)
    }

    async fn post<T: DeserializeOwned>(&self, url: &str, body: &Value) -> Result<T, PdpError> {
        tokio::time::timeout(self.timeout, self.send(url, body))
            .await
            .map_err(|_| PdpError::Timeout)?
    }

    async fn send<T: DeserializeOwned>(&self, url: &str, body: &Value) -> Result<T, PdpError> {
        let mut renewed = false;
        loop {
            let token = self.credentials.token().await.map_err(|error| {
                tracing::warn!(%error, "the policy service account could not get a token");
                match error {
                    crate::credentials::TokenError::Timeout => PdpError::Timeout,
                    crate::credentials::TokenError::Unavailable => PdpError::Unavailable,
                    crate::credentials::TokenError::Malformed => PdpError::Malformed,
                }
            })?;
            let mut response = self
                .http
                .post(url)
                .bearer_auth(&token)
                .json(body)
                .send()
                .await
                .map_err(|error| {
                    tracing::warn!(%error, "the policy call failed");
                    if error.is_timeout() {
                        PdpError::Timeout
                    } else {
                        PdpError::Unavailable
                    }
                })?;
            match response.status() {
                StatusCode::UNAUTHORIZED if !renewed => {
                    renewed = true;
                    self.credentials.forget();
                }
                status if status.is_success() => {
                    let mut bytes = Vec::new();
                    while let Some(chunk) =
                        response.chunk().await.map_err(|_| PdpError::Unavailable)?
                    {
                        if bytes.len() + chunk.len() > MAX_ANSWER_BYTES {
                            return Err(PdpError::Malformed);
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    return serde_json::from_slice(&bytes).map_err(|error| {
                        tracing::warn!(%error, "the policy answer is unreadable");
                        PdpError::Malformed
                    });
                }
                status => {
                    tracing::warn!(%status, "the policy service refused the evaluation");
                    return Err(PdpError::Unavailable);
                }
            }
        }
    }
}
