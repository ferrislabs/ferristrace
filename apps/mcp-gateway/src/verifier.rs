use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use serde_json::Value;
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

#[derive(Debug, Clone, PartialEq)]
pub struct Principal {
    pub sub: Option<String>,
    pub client_id: String,
    pub email: Option<String>,
    pub groups: Vec<String>,
    pub roles: Vec<String>,
    pub act: Option<Value>,
    pub delegated: bool,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("the token is not valid for this resource")]
    Invalid,
    #[error("the issuer's keys could not be read")]
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
#[error("the issuer is not a usable url")]
pub struct BadIssuer;

#[derive(Debug)]
pub struct JwksVerifier {
    issuer: String,
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
    azp: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    typ: Option<String>,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    sid: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    act: Option<Value>,
    #[serde(default)]
    groups: Option<Value>,
    #[serde(default)]
    roles: Option<Value>,
    #[serde(default)]
    realm_access: Option<Value>,
}

fn strings_of(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn roles_of(claims: &Claims) -> Vec<String> {
    let direct = strings_of(claims.roles.as_ref());
    if direct.is_empty() {
        strings_of(
            claims
                .realm_access
                .as_ref()
                .and_then(|realm| realm.get("roles")),
        )
    } else {
        direct
    }
}

fn present(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

impl JwksVerifier {
    pub fn new(issuer: &str) -> Result<Self, BadIssuer> {
        let issuer = issuer.trim_end_matches('/');
        if !(issuer.starts_with("https://") || issuer.starts_with("http://")) {
            return Err(BadIssuer);
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(FETCH_TIMEOUT)
            .build()
            .map_err(|_| BadIssuer)?;
        Ok(Self {
            issuer: issuer.to_owned(),
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

    pub async fn verify(&self, token: &str, audience: &str) -> Result<Principal, Refusal> {
        let header = decode_header(token).map_err(|_| Refusal::Invalid)?;
        if !matches!(header.alg, Algorithm::RS256 | Algorithm::ES256) {
            return Err(Refusal::Invalid);
        }
        let kid = header.kid.ok_or(Refusal::Invalid)?;
        let held = self.key_for(&kid).await?;
        if held.algorithm != header.alg {
            return Err(Refusal::Invalid);
        }

        let mut validation = Validation::new(header.alg);
        validation.leeway = LEEWAY_SECS;
        validation.validate_nbf = true;
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);
        let claims = decode::<Claims>(token, &held.key, &validation)
            .map_err(|_| Refusal::Invalid)?
            .claims;

        if claims
            .typ
            .as_deref()
            .is_some_and(|typ| !typ.eq_ignore_ascii_case(BEARER_TYPE))
        {
            return Err(Refusal::Invalid);
        }
        let roles = roles_of(&claims);
        let groups = strings_of(claims.groups.as_ref());
        let client_id = present(claims.azp)
            .or_else(|| present(claims.client_id))
            .ok_or(Refusal::Invalid)?;
        let sub = present(claims.sub);
        let delegated = sub.is_some() && present(claims.sid).is_some();
        Ok(Principal {
            sub,
            client_id,
            email: present(claims.email),
            groups,
            roles,
            act: claims.act.filter(Value::is_object),
            delegated,
            expires_at: claims.exp,
        })
    }

    async fn key_for(&self, kid: &str) -> Result<Arc<Held>, Refusal> {
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
                    Refusal::Invalid
                } else {
                    Refusal::Unavailable
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
        keys.held.get(kid).cloned().ok_or(Refusal::Invalid)
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
    ) -> Result<(String, HashMap<String, Arc<Held>>), Refusal> {
        let uri = if let Some(uri) = known {
            uri
        } else {
            self.discover().await?
        };
        let document = self.document(&uri).await?;
        let listed = document
            .get("keys")
            .and_then(Value::as_array)
            .ok_or(Refusal::Unavailable)?;
        Ok((uri, listed.iter().filter_map(held_from).collect()))
    }

    async fn discover(&self) -> Result<String, Refusal> {
        let url = format!("{}{DISCOVERY_PATH}", self.issuer);
        let body = self.document(&url).await?;
        let discovery: Discovery = serde_json::from_value(body).map_err(|error| {
            tracing::warn!(%error, "the issuer's discovery document is unusable");
            Refusal::Unavailable
        })?;
        if discovery.issuer.trim_end_matches('/') != self.issuer {
            tracing::warn!("the issuer's discovery document names another issuer");
            return Err(Refusal::Unavailable);
        }
        let issuer_host = reqwest::Url::parse(&self.issuer)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned));
        let keys_host = reqwest::Url::parse(&discovery.jwks_uri)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .and_then(|url| url.host_str().map(str::to_owned));
        if issuer_host.is_none() || issuer_host != keys_host {
            tracing::warn!("the issuer's keys are not served from the issuer's host");
            return Err(Refusal::Unavailable);
        }
        Ok(discovery.jwks_uri)
    }

    async fn document(&self, url: &str) -> Result<Value, Refusal> {
        let unreachable = |error: reqwest::Error| {
            tracing::warn!(%error, "the issuer did not answer");
            Refusal::Unavailable
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
            return Err(Refusal::Unavailable);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(unreachable)? {
            if body.len() + chunk.len() > MAX_DOCUMENT_BYTES {
                return Err(Refusal::Unavailable);
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| Refusal::Unavailable)
    }
}

fn held_from(listed: &Value) -> Option<(String, Arc<Held>)> {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn claims(value: &Value) -> Claims {
        serde_json::from_value(value.clone()).unwrap()
    }

    #[test]
    fn roles_come_from_the_roles_claim_or_the_realm_access() {
        let direct = claims(&json!({"exp": 1, "roles": ["sre"], "realm_access": {"roles": ["x"]}}));
        assert_eq!(roles_of(&direct), vec!["sre"]);
        let realm = claims(&json!({"exp": 1, "realm_access": {"roles": ["dev", 4, ""]}}));
        assert_eq!(roles_of(&realm), vec!["dev"]);
        assert!(roles_of(&claims(&json!({"exp": 1}))).is_empty());
    }

    #[test]
    fn a_set_that_is_not_an_array_of_strings_is_empty() {
        assert!(strings_of(Some(&json!("dev"))).is_empty());
        assert!(strings_of(Some(&json!({"a": 1}))).is_empty());
        assert_eq!(strings_of(Some(&json!(["a", 1, "b"]))), vec!["a", "b"]);
        assert!(strings_of(None).is_empty());
    }

    #[test]
    fn an_issuer_must_be_http_or_https() {
        assert!(JwksVerifier::new("https://auth.example/realms/r/").is_ok());
        assert!(JwksVerifier::new("ftp://auth.example").is_err());
        assert!(JwksVerifier::new("").is_err());
    }
}
