use super::errors::ReadRefusal;
use super::ports::ApiKeys;
use super::value_objects::{ReadGrant, Reader};
use crate::agents::{AgentCall, AgentGrant, AgentScope};
use crate::audit::{Action, Actor, AuditEntry, Detail, Target};
use crate::identity::SessionStore;
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Readers<S, G, K> {
    whoami: Whoami<S, G>,
    keys: K,
}

impl<S, G, K> Readers<S, G, K>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
{
    pub fn new(whoami: Whoami<S, G>, keys: K) -> Self {
        Self { whoami, keys }
    }

    pub fn whoami(&self) -> &Whoami<S, G> {
        &self.whoami
    }

    pub async fn grant(&self, reader: &Reader, org: TenantId) -> Result<ReadGrant, ReadRefusal> {
        match reader {
            Reader::Session(token) => {
                let who = self.whoami.of(token).await?;
                if who.may_act_in(org, OrgRole::Member) {
                    Ok(ReadGrant::whole(org))
                } else {
                    Err(ReadRefusal::NotYours)
                }
            }
            Reader::Key(secret) => {
                let grant = self
                    .keys
                    .grant_for(secret)
                    .await?
                    .ok_or(ReadRefusal::Unknown)?;
                if grant.org == org {
                    Ok(grant)
                } else {
                    Err(ReadRefusal::NotYours)
                }
            }
            Reader::Agent(call) => {
                let grant = self
                    .agent(call, org, Some(AgentScope::TelemetryRead))
                    .await?;
                Ok(ReadGrant {
                    org,
                    project: grant.project,
                })
            }
        }
    }

    pub async fn agent(
        &self,
        call: &AgentCall,
        org: TenantId,
        needs: Option<AgentScope>,
    ) -> Result<AgentGrant, ReadRefusal> {
        let grant = self.standing(call, org, needs).await?;
        let entry = AuditEntry {
            org,
            actor: Actor::Agent(grant.agent),
            action: Action::AgentRead,
            target: Target::org(org),
            detail: Detail::default()
                .with("route", &call.route)
                .with("jti", &call.token.jti),
        };
        self.record(&grant, &entry).await?;
        Ok(grant)
    }

    pub async fn standing(
        &self,
        call: &AgentCall,
        org: TenantId,
        needs: Option<AgentScope>,
    ) -> Result<AgentGrant, ReadRefusal> {
        let agent = self
            .keys
            .agent_standing(org, &call.token.client_id)
            .await?
            .ok_or(ReadRefusal::Unknown)?;
        if agent.is_revoked() {
            return Err(ReadRefusal::Unknown);
        }
        let scopes = agent.scopes.within(&call.token.scopes);
        let allowed = match needs {
            Some(scope) => scopes.contains(&scope),
            None => !scopes.is_empty(),
        };
        if !allowed {
            return Err(ReadRefusal::OutOfScope);
        }
        Ok(AgentGrant {
            agent: agent.id,
            name: agent.name,
            client_id: agent.client_id,
            org,
            scopes,
            project: agent.project,
            jti: call.token.jti.clone(),
        })
    }

    pub async fn record(&self, grant: &AgentGrant, entry: &AuditEntry) -> Result<(), ReadRefusal> {
        Ok(self.keys.agent_used(grant.org, grant.agent, entry).await?)
    }
}
