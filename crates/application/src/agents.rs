use ferristrace_domain::{
    AgentCall, AgentEnrolment, AgentError, AgentGrant, AgentId, AgentRegistry, AgentToken,
    ReadRefusal, Readers, RegisteredAgent, SessionToken, TenantId, TokenRefusal, VerifiedToken,
    Whoami,
};

use crate::registry::{AgentRepos, ReadRepos};
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S> {
    pub fn agents_enabled(&self) -> bool {
        self.agents.is_some()
    }

    pub async fn verify_agent(&self, token: &AgentToken) -> Result<VerifiedToken, TokenRefusal> {
        match &self.agents {
            Some(tokens) => ferristrace_domain::AgentTokens::verify(tokens, token).await,
            None => Err(TokenRefusal::Off),
        }
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: AgentRepos,
{
    #[transactional(sessions, memberships, agents, projects, audit, unavailable = AgentError::StoreUnreachable)]
    pub async fn list_agents(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<RegisteredAgent>, AgentError> {
        AgentRegistry::new(Whoami::new(sessions, memberships), agents, projects, audit)
            .list(token, org)
            .await
    }

    #[transactional(sessions, memberships, agents, projects, audit, unavailable = AgentError::StoreUnreachable)]
    pub async fn register_agent(
        &self,
        token: &SessionToken,
        org: TenantId,
        enrolment: &AgentEnrolment,
    ) -> Result<RegisteredAgent, AgentError> {
        AgentRegistry::new(Whoami::new(sessions, memberships), agents, projects, audit)
            .register(token, org, enrolment)
            .await
    }

    #[transactional(sessions, memberships, agents, projects, audit, unavailable = AgentError::StoreUnreachable)]
    pub async fn revoke_agent(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: AgentId,
    ) -> Result<(), AgentError> {
        AgentRegistry::new(Whoami::new(sessions, memberships), agents, projects, audit)
            .revoke(token, org, id)
            .await
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: ReadRepos,
{
    #[transactional(sessions, memberships, api_keys, unavailable = ReadRefusal::StoreUnreachable)]
    pub async fn agent_grant(
        &self,
        call: &AgentCall,
        org: TenantId,
    ) -> Result<AgentGrant, ReadRefusal> {
        Readers::new(Whoami::new(sessions, memberships), api_keys)
            .agent(call, org, None)
            .await
    }
}
