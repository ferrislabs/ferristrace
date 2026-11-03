use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::credentials::{ClientCredentials, TokenError, lifetime_of, token_request};
use crate::secret::Secret;

const SAFETY_MARGIN: Duration = Duration::from_secs(30);
const MAX_ENTRIES: usize = 4096;
const GRANT_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const TOKEN_TYPE_ACCESS: &str = "urn:ietf:params:oauth:token-type:access_token";

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[derive(Debug)]
pub struct Exchanger {
    http: reqwest::Client,
    token_url: String,
    client_id: String,
    client_secret: Secret,
    audience: String,
    timeout: Duration,
    actor: ClientCredentials,
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

impl Exchanger {
    pub fn new(
        http: reqwest::Client,
        token_url: String,
        client_id: String,
        client_secret: Secret,
        audience: String,
        timeout: Duration,
    ) -> Self {
        let actor = ClientCredentials::new(
            http.clone(),
            token_url.clone(),
            client_id.clone(),
            client_secret.clone(),
            None,
            timeout,
        );
        Self {
            http,
            token_url,
            client_id,
            client_secret,
            audience,
            timeout,
            actor,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub async fn exchange(
        &self,
        subject_token: &str,
        subject_expires_at: u64,
    ) -> Result<String, TokenError> {
        let now = Instant::now();
        if let Some(token) = self.cached(subject_token, now) {
            return Ok(token);
        }
        let actor_token = self.actor.token().await?;
        let answer = token_request(
            &self.http,
            &self.token_url,
            self.timeout,
            &self.client_id,
            &self.client_secret,
            vec![
                ("grant_type", GRANT_EXCHANGE),
                ("subject_token", subject_token),
                ("subject_token_type", TOKEN_TYPE_ACCESS),
                ("audience", &self.audience),
                ("actor_token", &actor_token),
                ("actor_token_type", TOKEN_TYPE_ACCESS),
            ],
        )
        .await?;
        let subject_left = Duration::from_secs(subject_expires_at.saturating_sub(unix_now()));
        let usable = lifetime_of(&answer)
            .min(subject_left)
            .saturating_sub(SAFETY_MARGIN);
        if !usable.is_zero() {
            self.keep(subject_token, &answer.access_token, now, now + usable);
        }
        Ok(answer.access_token)
    }

    fn cached(&self, subject_token: &str, now: Instant) -> Option<String> {
        let cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .get(subject_token)
            .filter(|(_, until)| now < *until)
            .map(|(token, _)| token.clone())
    }

    fn keep(&self, subject_token: &str, exchanged: &str, now: Instant, until: Instant) {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.retain(|_, (_, held_until)| now < *held_until);
        if cache.len() >= MAX_ENTRIES {
            cache.clear();
        }
        cache.insert(subject_token.to_owned(), (exchanged.to_owned(), until));
    }
}
