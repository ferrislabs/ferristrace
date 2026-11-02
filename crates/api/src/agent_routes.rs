use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use ferristrace_application::Stack;
use ferristrace_application::registry::{AgentRepos, ReadRepos};
use ferristrace_domain::{
    AccountId, AgentEnrolment, AgentError, AgentGrant, AgentId, AgentName, AgentScopes,
    BadAgentName, BadAgentScopes, BadClientId, ClientId, ProjectId, ReadRefusal, Reader,
    RegisteredAgent, TenantId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried, reading};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Serialize)]
pub struct Roll {
    enabled: bool,
    reason: Option<String>,
    agents: Vec<Registered>,
}

#[derive(Debug, Serialize)]
struct Registered {
    id: Uuid,
    name: String,
    client_id: String,
    scopes: Vec<&'static str>,
    project: Option<Uuid>,
    created_at_ms: u64,
    created_by: Option<Uuid>,
    last_used_at_ms: Option<u64>,
    revoked_at_ms: Option<u64>,
}

impl From<RegisteredAgent> for Registered {
    fn from(agent: RegisteredAgent) -> Self {
        Self {
            id: agent.id.as_uuid(),
            name: agent.name.into_string(),
            client_id: agent.client_id.into_string(),
            scopes: agent
                .scopes
                .as_slice()
                .iter()
                .map(|scope| scope.as_str())
                .collect(),
            project: agent.project.map(ProjectId::as_uuid),
            created_at_ms: agent.created_at_unix_nanos / NANOS_PER_MILLI,
            created_by: agent.created_by.map(AccountId::as_uuid),
            last_used_at_ms: agent
                .last_used_at_unix_nanos
                .map(|nanos| nanos / NANOS_PER_MILLI),
            revoked_at_ms: agent
                .revoked_at_unix_nanos
                .map(|nanos| nanos / NANOS_PER_MILLI),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Own {
    agent: Uuid,
    name: String,
    client_id: String,
    org: Uuid,
    scopes: Vec<&'static str>,
    project: Option<Uuid>,
    jti: String,
}

impl From<AgentGrant> for Own {
    fn from(grant: AgentGrant) -> Self {
        Self {
            agent: grant.agent.as_uuid(),
            name: grant.name.into_string(),
            client_id: grant.client_id.into_string(),
            org: grant.org.as_uuid(),
            scopes: grant.scopes.iter().map(|scope| scope.as_str()).collect(),
            project: grant.project.map(ProjectId::as_uuid),
            jti: grant.jti.into_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Asked {
    name: String,
    client_id: String,
    scopes: Vec<String>,
    #[serde(default)]
    project: Option<Uuid>,
}

pub fn agent_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: AgentRepos + ReadRepos,
{
    Router::new()
        .route("/api/orgs/{org}/agents", get(list::<S>).post(register::<S>))
        .route("/api/orgs/{org}/agents/me", get(mine::<S>))
        .route("/api/orgs/{org}/agents/{agent}", delete(revoke::<S>))
        .with_state(state)
}

async fn list<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Roll>, AgentRefused>
where
    S::Repos: AgentRepos + ReadRepos,
{
    let token = carried(&headers)?;
    let agents = state.app.list_agents(&token, TenantId::new(org)).await?;
    Ok(Json(Roll {
        enabled: state.app.agents_enabled(),
        reason: state.agents_off_reason.clone(),
        agents: agents.into_iter().map(Registered::from).collect(),
    }))
}

async fn register<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Asked>,
) -> Result<(StatusCode, Json<Registered>), AgentRefused>
where
    S::Repos: AgentRepos + ReadRepos,
{
    let token = carried(&headers)?;
    let client_id = ClientId::try_from(asked.client_id.as_str())?;
    if state.reserved_client_id.as_deref() == Some(client_id.as_str()) {
        return Err(AgentRefused::ReservedClientId);
    }
    let enrolment = AgentEnrolment {
        name: AgentName::try_from(asked.name.as_str())?,
        client_id,
        scopes: AgentScopes::parse(&asked.scopes)?,
        project: asked.project.map(ProjectId::new),
    };
    let agent = state
        .app
        .register_agent(&token, TenantId::new(org), &enrolment)
        .await?;
    tracing::info!(org = %org, agent = %agent.id, "an agent was registered");
    Ok((StatusCode::CREATED, Json(Registered::from(agent))))
}

async fn revoke<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, agent)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, AgentRefused>
where
    S::Repos: AgentRepos + ReadRepos,
{
    let token = carried(&headers)?;
    state
        .app
        .revoke_agent(&token, TenantId::new(org), AgentId::new(agent))
        .await?;
    tracing::info!(org = %org, agent = %agent, "an agent was revoked");
    Ok(StatusCode::NO_CONTENT)
}

async fn mine<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Own>, AgentRefused>
where
    S::Repos: AgentRepos + ReadRepos,
{
    let Reader::Agent(call) = reading(&state, &headers).await? else {
        return Err(AgentRefused::NotAnAgent);
    };
    let grant = state.app.agent_grant(&call, TenantId::new(org)).await?;
    Ok(Json(Own::from(grant)))
}

#[derive(Debug, thiserror::Error)]
pub enum AgentRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Name(#[from] BadAgentName),
    #[error(transparent)]
    ClientId(#[from] BadClientId),
    #[error(transparent)]
    Scopes(#[from] BadAgentScopes),
    #[error("that client id is the one people sign in to FerrisTrace with, not an agent")]
    ReservedClientId,
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("no such project")]
    NoSuchProject,
    #[error("no such agent")]
    NoSuchAgent,
    #[error("a live agent already carries that client id")]
    AlreadyRegistered,
    #[error("agents are an administrator's to register and revoke")]
    NotAnAdministrator,
    #[error("only an agent token answers here")]
    NotAnAgent,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error("the agents could not be read")]
    Unavailable,
}

impl From<AgentError> for AgentRefused {
    fn from(error: AgentError) -> Self {
        match error {
            AgentError::NoSession => Self::Reaching(NotCarried::NoSession),
            AgentError::NotYours => Self::NoSuchOrg,
            AgentError::NotYoursToChange => Self::NotAnAdministrator,
            AgentError::NoSuchProject => Self::NoSuchProject,
            AgentError::NoSuchAgent => Self::NoSuchAgent,
            AgentError::AlreadyRegistered => Self::AlreadyRegistered,
            AgentError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl From<ReadRefusal> for AgentRefused {
    fn from(error: ReadRefusal) -> Self {
        match error {
            ReadRefusal::Unknown => Self::Reaching(NotCarried::BadAgentToken),
            ReadRefusal::NotYours => Self::NoSuchOrg,
            ReadRefusal::OutOfScope => Self::OutOfScope,
            ReadRefusal::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for AgentRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Name(_) | Self::ClientId(_) | Self::Scopes(_) | Self::ReservedClientId => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::NoSuchOrg | Self::NoSuchProject | Self::NoSuchAgent => StatusCode::NOT_FOUND,
            Self::AlreadyRegistered => StatusCode::CONFLICT,
            Self::NotAnAdministrator | Self::NotAnAgent | Self::OutOfScope => StatusCode::FORBIDDEN,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the agents could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
