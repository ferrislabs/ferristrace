use super::entities::{Agent, AgentEnrolment};
use super::errors::AgentError;
use super::ports::Agents;
use super::value_objects::AgentId;
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target};
use crate::catalogue::Projects;
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct AgentRegistry<S, G, N, P, A> {
    whoami: Whoami<S, G>,
    agents: N,
    projects: P,
    trail: A,
}

impl<S, G, N, P, A> AgentRegistry<S, G, N, P, A>
where
    S: SessionStore,
    G: Memberships,
    N: Agents,
    P: Projects,
    A: AuditTrail,
{
    pub fn new(whoami: Whoami<S, G>, agents: N, projects: P, trail: A) -> Self {
        Self {
            whoami,
            agents,
            projects,
            trail,
        }
    }

    pub async fn list(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<Agent>, AgentError> {
        self.standing(token, org).await?;
        Ok(self.agents.list(org).await?)
    }

    pub async fn register(
        &self,
        token: &SessionToken,
        org: TenantId,
        enrolment: &AgentEnrolment,
    ) -> Result<Agent, AgentError> {
        let by = self.standing(token, org).await?;
        if let Some(project) = enrolment.project {
            let held = self.projects.of(org).await?;
            if !held.iter().any(|kept| kept.id == project) {
                return Err(AgentError::NoSuchProject);
            }
        }
        let agent = self
            .agents
            .register(org, enrolment, by)
            .await?
            .ok_or(AgentError::AlreadyRegistered)?;
        self.note(
            org,
            by,
            Action::AgentRegistered,
            agent.id,
            Detail::default()
                .with("name", enrolment.name.as_str())
                .with("client_id", &enrolment.client_id)
                .with_all("scopes", enrolment.scopes.as_slice())
                .with_maybe("project", enrolment.project),
        )
        .await?;
        Ok(agent)
    }

    pub async fn revoke(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: AgentId,
    ) -> Result<(), AgentError> {
        let by = self.standing(token, org).await?;
        let agent = self
            .agents
            .revoke(org, id)
            .await?
            .ok_or(AgentError::NoSuchAgent)?;
        self.note(
            org,
            by,
            Action::AgentRevoked,
            id,
            Detail::default()
                .with("name", &agent.name)
                .with("client_id", &agent.client_id),
        )
        .await
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        agent: AgentId,
        detail: Detail,
    ) -> Result<(), AgentError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(by),
                action,
                target: Target::agent(agent),
                detail,
            })
            .await?)
    }

    async fn standing(&self, token: &SessionToken, org: TenantId) -> Result<AccountId, AgentError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(AgentError::NotYours),
            Some(role) if !role.at_least(OrgRole::Admin) => Err(AgentError::NotYoursToChange),
            Some(_) => Ok(who.account),
        }
    }
}
