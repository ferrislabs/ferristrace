use super::value_objects::{AgentId, AgentName, AgentScope, AgentScopes, ClientId};
use crate::catalogue::ProjectId;
use crate::identity::AccountId;
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub id: AgentId,
    pub org: TenantId,
    pub name: Box<str>,
    pub client_id: Box<str>,
    pub scopes: AgentScopes,
    pub project: Option<ProjectId>,
    pub created_by: Option<AccountId>,
    pub created_at_unix_nanos: u64,
    pub last_used_at_unix_nanos: Option<u64>,
    pub revoked_at_unix_nanos: Option<u64>,
}

impl Agent {
    pub fn is_revoked(&self) -> bool {
        self.revoked_at_unix_nanos.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEnrolment {
    pub name: AgentName,
    pub client_id: ClientId,
    pub scopes: AgentScopes,
    pub project: Option<ProjectId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentGrant {
    pub agent: AgentId,
    pub name: Box<str>,
    pub client_id: Box<str>,
    pub org: TenantId,
    pub scopes: Vec<AgentScope>,
    pub project: Option<ProjectId>,
    pub jti: Box<str>,
}
