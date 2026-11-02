use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::{AgentRepos, ReadRepos};
use ferristrace_domain::{
    AgentEnrolment, AgentError, AgentName, AgentScope, AgentScopes, BadAgentScopes, ClientId,
    ProjectId, RegisteredAgent, TenantId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent_routes::{AgentRefused, Registered};
use crate::carried::{NotCarried, carried};
use crate::state::AppState;

const GATEWAY_NAME: &str = "MCP gateway";

#[derive(Debug, Serialize)]
pub struct McpState {
    configured: bool,
    agents_enabled: bool,
    agents_off_reason: Option<String>,
    gateway_client_id: Option<String>,
    url: Option<String>,
    enabled: bool,
    agent: Option<Registered>,
}

#[derive(Debug, Deserialize)]
pub struct Turning {
    scopes: Vec<String>,
    #[serde(default)]
    project: Option<Uuid>,
}

pub fn mcp_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: AgentRepos + ReadRepos,
{
    Router::new()
        .route(
            "/api/orgs/{org}/mcp",
            get(shown::<S>).put(turn_on::<S>).delete(turn_off::<S>),
        )
        .with_state(state)
}

fn url_of(public_url: &str, org: Uuid) -> String {
    format!("{}/orgs/{org}/mcp", public_url.trim_end_matches('/'))
}

fn gateway_of<S: Stack>(state: &AppState<S>) -> Result<(&str, &str), McpRefused> {
    match (
        state.mcp.gateway_client_id.as_deref(),
        state.mcp.public_url.as_deref(),
    ) {
        (Some(client), Some(url)) if !client.is_empty() && !url.is_empty() => Ok((client, url)),
        _ => Err(McpRefused::NotConfigured),
    }
}

fn live<'a>(agents: &'a [RegisteredAgent], client_id: &str) -> Option<&'a RegisteredAgent> {
    agents
        .iter()
        .find(|agent| &*agent.client_id == client_id && !agent.is_revoked())
}

async fn shown<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<McpState>, McpRefused>
where
    S::Repos: AgentRepos + ReadRepos,
{
    let token = carried(&headers)?;
    let agents = state.app.list_agents(&token, TenantId::new(org)).await?;
    let gateway = gateway_of(&state).ok();
    let agent = gateway
        .and_then(|(client, _)| live(&agents, client))
        .cloned()
        .map(Registered::from);
    Ok(Json(McpState {
        configured: gateway.is_some(),
        agents_enabled: state.app.agents_enabled(),
        agents_off_reason: state.agents_off_reason.clone(),
        gateway_client_id: state.mcp.gateway_client_id.clone(),
        url: gateway.map(|(_, public)| url_of(public, org)),
        enabled: agent.is_some(),
        agent,
    }))
}

async fn turn_on<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Turning>,
) -> Result<(StatusCode, Json<Registered>), McpRefused>
where
    S::Repos: AgentRepos + ReadRepos,
{
    let token = carried(&headers)?;
    let (gateway, _) = gateway_of(&state)?;
    if state.reserved_client_id.as_deref() == Some(gateway) {
        return Err(AgentRefused::ReservedClientId.into());
    }
    let scopes = AgentScopes::parse(&asked.scopes)?;
    if !scopes.contains(AgentScope::TelemetryRead) {
        return Err(McpRefused::NeedsTelemetry);
    }
    let project = asked.project.map(ProjectId::new);
    let org_id = TenantId::new(org);
    let held = state.app.list_agents(&token, org_id).await?;
    if let Some(agent) = live(&held, gateway) {
        if agent.scopes == scopes && agent.project == project {
            return Ok((StatusCode::OK, Json(Registered::from(agent.clone()))));
        }
        state.app.revoke_agent(&token, org_id, agent.id).await?;
    }
    let enrolment = AgentEnrolment {
        name: AgentName::try_from(GATEWAY_NAME).map_err(AgentRefused::from)?,
        client_id: ClientId::try_from(gateway).map_err(AgentRefused::from)?,
        scopes,
        project,
    };
    let agent = state.app.register_agent(&token, org_id, &enrolment).await?;
    tracing::info!(org = %org, agent = %agent.id, "mcp was turned on");
    Ok((StatusCode::CREATED, Json(Registered::from(agent))))
}

async fn turn_off<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<StatusCode, McpRefused>
where
    S::Repos: AgentRepos + ReadRepos,
{
    let token = carried(&headers)?;
    let (gateway, _) = gateway_of(&state)?;
    let org_id = TenantId::new(org);
    let held = state.app.list_agents(&token, org_id).await?;
    let agent = live(&held, gateway).ok_or(McpRefused::NotOn)?;
    state.app.revoke_agent(&token, org_id, agent.id).await?;
    tracing::info!(org = %org, agent = %agent.id, "mcp was turned off");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, thiserror::Error)]
pub enum McpRefused {
    #[error(transparent)]
    Agent(#[from] AgentRefused),
    #[error(transparent)]
    Scopes(#[from] BadAgentScopes),
    #[error(
        "MCP is not configured on this server: set FERRISTRACE_MCP__GATEWAY_CLIENT_ID and FERRISTRACE_MCP__PUBLIC_URL"
    )]
    NotConfigured,
    #[error("the gateway reads FerrisTrace with telemetry:read, so MCP keeps that scope")]
    NeedsTelemetry,
    #[error("MCP is not on for this organisation")]
    NotOn,
}

impl From<NotCarried> for McpRefused {
    fn from(error: NotCarried) -> Self {
        Self::Agent(AgentRefused::Reaching(error))
    }
}

impl From<AgentError> for McpRefused {
    fn from(error: AgentError) -> Self {
        Self::Agent(AgentRefused::from(error))
    }
}

impl IntoResponse for McpRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Agent(refused) => return refused.into_response(),
            Self::Scopes(_) | Self::NeedsTelemetry => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NotConfigured => StatusCode::CONFLICT,
            Self::NotOn => StatusCode::NOT_FOUND,
        };
        (status, self.to_string()).into_response()
    }
}
