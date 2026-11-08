use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use ferristrace_application::Stack;
use ferristrace_application::registry::AccessRepos;
use ferristrace_domain::{
    AccountId, ApiKey, BadKeyName, BadScopes, IssuedKey, KeyError, KeyId, KeyName, KeyScopes,
    KeyTtl, ProjectId, TenantId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Serialize)]
pub struct Ring {
    keys: Vec<Held>,
}

#[derive(Debug, Serialize)]
struct Held {
    id: Uuid,
    name: String,
    prefix: String,
    project: Option<Uuid>,
    scopes: Vec<&'static str>,
    created_at_ms: u64,
    created_by: Option<Uuid>,
    last_used_at_ms: Option<u64>,
    expires_at_ms: Option<u64>,
}

impl From<ApiKey> for Held {
    fn from(key: ApiKey) -> Self {
        Self {
            id: key.id.as_uuid(),
            name: key.name.into_string(),
            prefix: key.prefix.into_string(),
            project: key.project.map(ProjectId::as_uuid),
            scopes: key
                .scopes
                .as_slice()
                .iter()
                .map(|scope| scope.as_str())
                .collect(),
            created_at_ms: key.created_at_unix_nanos / NANOS_PER_MILLI,
            created_by: key.created_by.map(AccountId::as_uuid),
            last_used_at_ms: key
                .last_used_at_unix_nanos
                .map(|nanos| nanos / NANOS_PER_MILLI),
            expires_at_ms: key
                .expires_at_unix_nanos
                .map(|nanos| nanos / NANOS_PER_MILLI),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Issued {
    key: Held,
    secret: String,
}

impl From<IssuedKey> for Issued {
    fn from(issued: IssuedKey) -> Self {
        Self {
            secret: issued.secret.exposed().to_owned(),
            key: Held::from(issued.key),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Asked {
    name: String,
    scopes: Vec<String>,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    ttl: Option<String>,
}

pub fn key_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: AccessRepos,
{
    Router::new()
        .route("/api/orgs/{org}/keys", get(list::<S>).post(issue::<S>))
        .route("/api/orgs/{org}/keys/{key}", delete(revoke::<S>))
        .with_state(state)
}

async fn list<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Ring>, KeyRefused>
where
    S::Repos: AccessRepos,
{
    let token = carried(&state, &headers).await?;
    let keys = state.app.list_keys(&token, TenantId::new(org)).await?;
    Ok(Json(Ring {
        keys: keys.into_iter().map(Held::from).collect(),
    }))
}

async fn issue<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Asked>,
) -> Result<(StatusCode, Json<Issued>), KeyRefused>
where
    S::Repos: AccessRepos,
{
    let token = carried(&state, &headers).await?;
    let name = KeyName::try_from(asked.name.as_str())?;
    let scopes = KeyScopes::parse(&asked.scopes)?;
    let ttl = match &asked.ttl {
        None => KeyTtl::Never,
        Some(spelling) => KeyTtl::parse(spelling).ok_or(KeyRefused::Ttl)?,
    };
    let issued = state
        .app
        .issue_key(
            &token,
            TenantId::new(org),
            asked.project.map(ProjectId::new),
            &name,
            &scopes,
            ttl,
        )
        .await?;
    tracing::info!(org = %org, key = %issued.key.id, "a key was issued");
    Ok((StatusCode::CREATED, Json(Issued::from(issued))))
}

async fn revoke<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, key)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, KeyRefused>
where
    S::Repos: AccessRepos,
{
    let token = carried(&state, &headers).await?;
    state
        .app
        .revoke_key(&token, TenantId::new(org), KeyId::new(key))
        .await?;
    tracing::info!(org = %org, key = %key, "a key was revoked");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, thiserror::Error)]
pub enum KeyRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Name(#[from] BadKeyName),
    #[error(transparent)]
    Scopes(#[from] BadScopes),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("no such project")]
    NoSuchProject,
    #[error("no such key")]
    NoSuchKey,
    #[error("that is not a ttl this organisation offers")]
    Ttl,
    #[error("keys are an administrator's to issue and revoke")]
    NotAnAdministrator,
    #[error("the keys could not be read")]
    Unavailable,
}

impl From<KeyError> for KeyRefused {
    fn from(error: KeyError) -> Self {
        match error {
            KeyError::NoSession => Self::Reaching(NotCarried::NoSession),
            KeyError::NotYours => Self::NoSuchOrg,
            KeyError::NotYoursToChange => Self::NotAnAdministrator,
            KeyError::NoSuchProject => Self::NoSuchProject,
            KeyError::NoSuchKey => Self::NoSuchKey,
            KeyError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for KeyRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Name(_) | Self::Scopes(_) | Self::Ttl => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NoSuchOrg | Self::NoSuchProject | Self::NoSuchKey => StatusCode::NOT_FOUND,
            Self::NotAnAdministrator => StatusCode::FORBIDDEN,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the keys could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
