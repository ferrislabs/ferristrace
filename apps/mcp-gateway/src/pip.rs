use std::time::{Duration, Instant};

use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::cache::OrgCache;
use crate::credentials::ClientCredentials;

const MAX_ANSWER_BYTES: usize = 1 << 20;
const ZONE_TTL: Duration = Duration::from_secs(300);
const ONCALL_TTL: Duration = Duration::from_secs(30);
const TARGET_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OnCall {
    pub on_call: bool,
    #[serde(default)]
    pub schedules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Incident {
    pub severity: String,
    pub status: String,
    #[serde(default)]
    pub services: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Placement {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub environments: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ClockAnswer {
    timezone: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Unavailable {
    #[error("a fact the policies need could not be read from FerrisTrace")]
    Failed,
    #[error("FerrisTrace does not know the incident or service the call names")]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Needs<'a> {
    pub oncall_email: Option<&'a str>,
    pub incident: Option<Uuid>,
    pub service: Option<&'a str>,
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub timezone: String,
    pub oncall: Option<OnCall>,
    pub incident: Option<Incident>,
    pub placement: Option<Placement>,
}

#[derive(Debug)]
pub struct Pip {
    http: reqwest::Client,
    api: Url,
    credentials: ClientCredentials,
    budget: Duration,
    zones: OrgCache<String>,
    oncall: OrgCache<OnCall>,
    incidents: OrgCache<Option<Incident>>,
    placements: OrgCache<Option<Placement>>,
}

impl Pip {
    pub fn new(
        http: reqwest::Client,
        api: Url,
        credentials: ClientCredentials,
        budget: Duration,
    ) -> Self {
        Self {
            http,
            api,
            credentials,
            budget,
            zones: OrgCache::new(ZONE_TTL),
            oncall: OrgCache::new(ONCALL_TTL),
            incidents: OrgCache::new(TARGET_TTL),
            placements: OrgCache::new(TARGET_TTL),
        }
    }

    pub async fn gather(&self, org: Uuid, needs: &Needs<'_>) -> Result<Facts, Unavailable> {
        tokio::time::timeout(self.budget, self.collect(org, needs))
            .await
            .map_err(|_| {
                tracing::warn!(%org, "the lookups did not finish within the budget");
                Unavailable::Failed
            })?
    }

    async fn collect(&self, org: Uuid, needs: &Needs<'_>) -> Result<Facts, Unavailable> {
        let (timezone, oncall, incident) = tokio::join!(
            self.timezone(org),
            self.oncall(org, needs.oncall_email),
            self.incident(org, needs.incident, needs.fresh),
        );
        let timezone = timezone?;
        let oncall = oncall?;
        let incident = incident?;
        let service = needs.service.map(str::to_owned).or_else(|| {
            incident
                .as_ref()
                .and_then(|incident| match incident.services.as_slice() {
                    [only] => Some(only.clone()),
                    _ => None,
                })
        });
        let placement = match service {
            Some(service) => self.placement(org, &service, needs.fresh).await?,
            None => None,
        };
        Ok(Facts {
            timezone,
            oncall,
            incident,
            placement,
        })
    }

    async fn timezone(&self, org: Uuid) -> Result<String, Unavailable> {
        let now = Instant::now();
        if let Some(zone) = self.zones.get(org, "", now) {
            return Ok(zone);
        }
        let url = self.url(org, &["clock"]);
        let answer: ClockAnswer = self.fetch(url).await?.ok_or(Unavailable::Failed)?;
        self.zones
            .put(org, String::new(), answer.timezone.clone(), now);
        Ok(answer.timezone)
    }

    async fn oncall(&self, org: Uuid, email: Option<&str>) -> Result<Option<OnCall>, Unavailable> {
        let Some(email) = email else {
            return Ok(None);
        };
        let now = Instant::now();
        let key = email.to_ascii_lowercase();
        if let Some(held) = self.oncall.get(org, &key, now) {
            return Ok(Some(held));
        }
        let mut url = self.url(org, &["oncall", "now"]);
        url.query_pairs_mut().append_pair("email", email);
        let answer: OnCall = self.fetch(url).await?.ok_or(Unavailable::Failed)?;
        self.oncall.put(org, key, answer.clone(), now);
        Ok(Some(answer))
    }

    async fn incident(
        &self,
        org: Uuid,
        id: Option<Uuid>,
        fresh: bool,
    ) -> Result<Option<Incident>, Unavailable> {
        let Some(id) = id else {
            return Ok(None);
        };
        let now = Instant::now();
        if !fresh && let Some(held) = self.incidents.get(org, &id.to_string(), now) {
            return held.map(Some).ok_or(Unavailable::Unknown);
        }
        let url = self.url(org, &["incidents", &id.to_string()]);
        let answer: Option<Incident> = self.fetch(url).await?;
        self.incidents.put(org, id.to_string(), answer.clone(), now);
        answer.map(Some).ok_or(Unavailable::Unknown)
    }

    async fn placement(
        &self,
        org: Uuid,
        service: &str,
        fresh: bool,
    ) -> Result<Option<Placement>, Unavailable> {
        let now = Instant::now();
        if !fresh && let Some(held) = self.placements.get(org, service, now) {
            return held.map(Some).ok_or(Unavailable::Unknown);
        }
        let url = self.url(org, &["services", service, "placement"]);
        let answer: Option<Placement> = self.fetch(url).await?;
        self.placements
            .put(org, service.to_owned(), answer.clone(), now);
        answer.map(Some).ok_or(Unavailable::Unknown)
    }

    fn url(&self, org: Uuid, tail: &[&str]) -> Url {
        let mut url = self.api.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty()
                .extend(["api", "orgs", &org.to_string()])
                .extend(tail);
        }
        url
    }

    async fn fetch<T: DeserializeOwned>(&self, url: Url) -> Result<Option<T>, Unavailable> {
        let mut renewed = false;
        loop {
            let token = self.credentials.token().await.map_err(|error| {
                tracing::warn!(%error, "the gateway could not get its own token");
                Unavailable::Failed
            })?;
            let mut response = self
                .http
                .get(url.clone())
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|error| {
                    tracing::warn!(%error, path = url.path(), "a lookup failed");
                    Unavailable::Failed
                })?;
            match response.status() {
                StatusCode::UNAUTHORIZED if !renewed => {
                    renewed = true;
                    self.credentials.forget();
                }
                StatusCode::NOT_FOUND => return Ok(None),
                status if status.is_success() => {
                    if response
                        .content_length()
                        .is_some_and(|length| length > MAX_ANSWER_BYTES as u64)
                    {
                        return Err(Unavailable::Failed);
                    }
                    let mut body = Vec::new();
                    while let Some(chunk) =
                        response.chunk().await.map_err(|_| Unavailable::Failed)?
                    {
                        if body.len() + chunk.len() > MAX_ANSWER_BYTES {
                            return Err(Unavailable::Failed);
                        }
                        body.extend_from_slice(&chunk);
                    }
                    return serde_json::from_slice(&body).map(Some).map_err(|error| {
                        tracing::warn!(%error, path = url.path(), "a lookup answered unreadably");
                        Unavailable::Failed
                    });
                }
                status => {
                    tracing::warn!(%status, path = url.path(), "a lookup was refused");
                    return Err(Unavailable::Failed);
                }
            }
        }
    }
}
