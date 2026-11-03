use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ferristrace_domain::{AgentToken, AgentTokens, Delegation, TokenRefusal, VerifiedToken};
use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::Instant;

const DISCOVERY_PATH: &str = "/.well-known/openid-configuration";
const KEYS_LIVE_FOR: Duration = Duration::from_secs(600);
const REFETCH_AT_MOST_EVERY: Duration = Duration::from_secs(60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const LEEWAY_SECS: u64 = 30;
const MAX_DOCUMENT_BYTES: usize = 1 << 20;
const MIN_MODULUS_CHARS: usize = 340;
const BEARER_TYPE: &str = "Bearer";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JwksSettings {
    pub issuer: String,
    pub audience: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum JwksError {
    #[error("the issuer is not a usable url")]
    Settings,
}

#[derive(Debug)]
pub struct JwksVerifier {
    settings: JwksSettings,
    http: reqwest::Client,
    live_for: Duration,
    refetch_gap: Duration,
    state: Mutex<Keys>,
    flight: Mutex<()>,
}

#[derive(Debug, Default)]
struct Keys {
    jwks_uri: Option<String>,
    held: HashMap<String, Arc<Held>>,
    fetched_at: Option<Instant>,
    attempted_at: Option<Instant>,
}

#[derive(Debug)]
struct Held {
    key: DecodingKey,
    algorithm: Algorithm,
}

#[derive(Debug, Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
}

#[derive(Debug, Deserialize)]
struct Claims {
    exp: u64,
    #[serde(default)]
    jti: Option<String>,
    #[serde(default)]
    azp: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    typ: Option<String>,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    act: Option<serde_json::Value>,
}

fn delegation_of(
    sub: Option<String>,
    email: Option<String>,
    act: Option<serde_json::Value>,
) -> Result<Option<Delegation>, TokenRefusal> {
    let Some(act) = act else {
        return Ok(None);
    };
    let serde_json::Value::Object(act) = act else {
        return Err(TokenRefusal::Invalid);
    };
    let subject = sub
        .filter(|sub| !sub.is_empty())
        .ok_or(TokenRefusal::Invalid)?;
    let named = |key: &str| {
        act.get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|held| !held.is_empty())
            .map(Box::<str>::from)
    };
    Ok(Some(Delegation {
        subject: subject.into(),
        email: email.filter(|email| !email.is_empty()).map(Into::into),
        act_sub: named("sub"),
        act_client_id: named("client_id"),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Scope {
    Spaced(String),
    Listed(Vec<String>),
}

impl Scope {
    fn into_scopes(self) -> Vec<Box<str>> {
        match self {
            Self::Spaced(spaced) => spaced.split_whitespace().map(Into::into).collect(),
            Self::Listed(listed) => listed.into_iter().map(String::into_boxed_str).collect(),
        }
    }
}

impl JwksVerifier {
    pub fn new(settings: JwksSettings) -> Result<Self, JwksError> {
        let issuer = settings.issuer.trim_end_matches('/');
        if !(issuer.starts_with("https://") || issuer.starts_with("http://")) {
            return Err(JwksError::Settings);
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(FETCH_TIMEOUT)
            .build()
            .map_err(|_| JwksError::Settings)?;
        Ok(Self {
            settings: JwksSettings {
                issuer: issuer.to_owned(),
                audience: settings.audience.filter(|audience| !audience.is_empty()),
            },
            http,
            live_for: KEYS_LIVE_FOR,
            refetch_gap: REFETCH_AT_MOST_EVERY,
            state: Mutex::new(Keys::default()),
            flight: Mutex::new(()),
        })
    }

    #[must_use]
    pub fn with_cache_timing(mut self, live_for: Duration, refetch_gap: Duration) -> Self {
        self.live_for = live_for;
        self.refetch_gap = refetch_gap;
        self
    }

    async fn key_for(&self, kid: &str) -> Result<Arc<Held>, TokenRefusal> {
        let now = Instant::now();
        if let Some(held) = self.fresh(kid, now).await {
            return Ok(held);
        }
        let _flight = if let Ok(flight) = self.flight.try_lock() {
            flight
        } else {
            if let Some(held) = self.state.lock().await.held.get(kid).cloned() {
                return Ok(held);
            }
            self.flight.lock().await
        };
        let uri = {
            let mut keys = self.state.lock().await;
            let fresh = keys
                .fetched_at
                .is_some_and(|at| now.duration_since(at) < self.live_for);
            if fresh && let Some(held) = keys.held.get(kid) {
                return Ok(Arc::clone(held));
            }
            let may_fetch = keys
                .attempted_at
                .is_none_or(|at| now.duration_since(at) >= self.refetch_gap);
            if !may_fetch {
                return keys.held.get(kid).cloned().ok_or(if fresh {
                    TokenRefusal::Invalid
                } else {
                    TokenRefusal::Unavailable
                });
            }
            keys.attempted_at = Some(now);
            keys.jwks_uri.clone()
        };
        let fetched = self.fetch(uri).await;
        let mut keys = self.state.lock().await;
        match fetched {
            Ok((uri, held)) => {
                keys.jwks_uri = Some(uri);
                keys.held = held;
                keys.fetched_at = Some(now);
            }
            Err(refusal) => return keys.held.get(kid).cloned().ok_or(refusal),
        }
        keys.held.get(kid).cloned().ok_or(TokenRefusal::Invalid)
    }

    async fn fresh(&self, kid: &str, now: Instant) -> Option<Arc<Held>> {
        let keys = self.state.lock().await;
        let fresh = keys
            .fetched_at
            .is_some_and(|at| now.duration_since(at) < self.live_for);
        if fresh {
            keys.held.get(kid).cloned()
        } else {
            None
        }
    }

    async fn fetch(
        &self,
        known: Option<String>,
    ) -> Result<(String, HashMap<String, Arc<Held>>), TokenRefusal> {
        let uri = if let Some(uri) = known {
            uri
        } else {
            self.discover().await?
        };
        let document = self.document(&uri).await?;
        let listed = document
            .get("keys")
            .and_then(serde_json::Value::as_array)
            .ok_or(TokenRefusal::Unavailable)?;
        Ok((uri, listed.iter().filter_map(held_from).collect()))
    }

    async fn discover(&self) -> Result<String, TokenRefusal> {
        let url = format!("{}{DISCOVERY_PATH}", self.settings.issuer);
        let body = self.document(&url).await?;
        let discovery: Discovery = serde_json::from_value(body).map_err(|error| {
            tracing::warn!(%error, "the agent issuer's discovery document is unusable");
            TokenRefusal::Unavailable
        })?;
        if discovery.issuer.trim_end_matches('/') != self.settings.issuer {
            tracing::warn!("the agent issuer's discovery document names another issuer");
            return Err(TokenRefusal::Unavailable);
        }
        let issuer_host = reqwest::Url::parse(&self.settings.issuer)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned));
        let keys_host = reqwest::Url::parse(&discovery.jwks_uri)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .and_then(|url| url.host_str().map(str::to_owned));
        if issuer_host.is_none() || issuer_host != keys_host {
            tracing::warn!("the agent issuer's keys are not served from the issuer's host");
            return Err(TokenRefusal::Unavailable);
        }
        Ok(discovery.jwks_uri)
    }

    async fn document(&self, url: &str) -> Result<serde_json::Value, TokenRefusal> {
        let unreachable = |error: reqwest::Error| {
            tracing::warn!(%error, "the agent issuer did not answer");
            TokenRefusal::Unavailable
        };
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unreachable)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_DOCUMENT_BYTES as u64)
        {
            return Err(TokenRefusal::Unavailable);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(unreachable)? {
            if body.len() + chunk.len() > MAX_DOCUMENT_BYTES {
                return Err(TokenRefusal::Unavailable);
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| TokenRefusal::Unavailable)
    }
}

fn held_from(listed: &serde_json::Value) -> Option<(String, Arc<Held>)> {
    let jwk: Jwk = serde_json::from_value(listed.clone()).ok()?;
    let kid = jwk.common.key_id.clone()?;
    if matches!(jwk.common.public_key_use, Some(PublicKeyUse::Encryption)) {
        return None;
    }
    let algorithm = match &jwk.algorithm {
        AlgorithmParameters::RSA(params) if params.n.len() >= MIN_MODULUS_CHARS => Algorithm::RS256,
        AlgorithmParameters::EllipticCurve(params) if params.curve == EllipticCurve::P256 => {
            Algorithm::ES256
        }
        _ => return None,
    };
    let key = DecodingKey::from_jwk(&jwk).ok()?;
    Some((kid, Arc::new(Held { key, algorithm })))
}

impl AgentTokens for JwksVerifier {
    async fn verify(&self, token: &AgentToken) -> Result<VerifiedToken, TokenRefusal> {
        let header = decode_header(token.exposed()).map_err(|_| TokenRefusal::Invalid)?;
        if !matches!(header.alg, Algorithm::RS256 | Algorithm::ES256) {
            return Err(TokenRefusal::Invalid);
        }
        let kid = header.kid.ok_or(TokenRefusal::Invalid)?;
        let held = self.key_for(&kid).await?;
        if held.algorithm != header.alg {
            return Err(TokenRefusal::Invalid);
        }

        let mut validation = Validation::new(header.alg);
        validation.leeway = LEEWAY_SECS;
        validation.validate_nbf = true;
        validation.set_issuer(&[&self.settings.issuer]);
        validation.set_required_spec_claims(&["exp", "iss"]);
        match &self.settings.audience {
            Some(audience) => {
                validation.set_audience(&[audience]);
                validation.required_spec_claims.insert("aud".to_owned());
            }
            None => validation.validate_aud = false,
        }
        let claims = decode::<Claims>(token.exposed(), &held.key, &validation)
            .map_err(|_| TokenRefusal::Invalid)?
            .claims;

        if claims
            .typ
            .as_deref()
            .is_some_and(|typ| !typ.eq_ignore_ascii_case(BEARER_TYPE))
        {
            return Err(TokenRefusal::Invalid);
        }
        let jti = claims
            .jti
            .filter(|jti| !jti.is_empty())
            .ok_or(TokenRefusal::Invalid)?;
        let client_id = claims
            .azp
            .filter(|azp| !azp.is_empty())
            .or_else(|| claims.client_id.filter(|client| !client.is_empty()))
            .ok_or(TokenRefusal::Invalid)?;
        let delegated = delegation_of(claims.sub, claims.email, claims.act)?;
        Ok(VerifiedToken {
            client_id: client_id.into(),
            scopes: claims.scope.map(Scope::into_scopes).unwrap_or_default(),
            jti: jti.into(),
            expires_at_unix_secs: claims.exp,
            delegated,
        })
    }
}
