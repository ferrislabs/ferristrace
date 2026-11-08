use super::entities::{ApiKey, IssuedKey};
use super::errors::KeyError;
use super::ports::ApiKeys;
use super::value_objects::{KeyId, KeyName, KeyScopes, KeyTtl};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target};
use crate::catalogue::{ProjectId, Projects};
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct KeyRing<S, G, K, P, A> {
    whoami: Whoami<S, G>,
    keys: K,
    projects: P,
    trail: A,
}

impl<S, G, K, P, A> KeyRing<S, G, K, P, A>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    P: Projects,
    A: AuditTrail,
{
    pub fn new(whoami: Whoami<S, G>, keys: K, projects: P, trail: A) -> Self {
        Self {
            whoami,
            keys,
            projects,
            trail,
        }
    }

    pub async fn list(&self, token: &SessionToken, org: TenantId) -> Result<Vec<ApiKey>, KeyError> {
        self.standing(token, org, OrgRole::Member).await?;
        Ok(self.keys.list(org).await?)
    }

    pub async fn issue(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        ttl: KeyTtl,
    ) -> Result<IssuedKey, KeyError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        if let Some(project) = project {
            let held = self.projects.of(org).await?;
            if !held.iter().any(|kept| kept.id == project) {
                return Err(KeyError::NoSuchProject);
            }
        }
        let issued = self.keys.issue(org, project, name, scopes, by, ttl).await?;
        self.note(
            org,
            by,
            Action::KeyIssued,
            issued.key.id,
            Detail::default()
                .with("name", name.as_str())
                .with_all("scopes", scopes.as_slice())
                .with_maybe("project", project)
                .with("ttl", ttl.as_str()),
        )
        .await?;
        Ok(issued)
    }

    pub async fn revoke(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: KeyId,
    ) -> Result<(), KeyError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        let name = self
            .keys
            .list(org)
            .await?
            .into_iter()
            .find(|key| key.id == id)
            .map(|key| key.name);
        if !self.keys.revoke(org, id).await? {
            return Err(KeyError::NoSuchKey);
        }
        self.note(
            org,
            by,
            Action::KeyRevoked,
            id,
            Detail::default().with_maybe("name", name),
        )
        .await
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        key: KeyId,
        detail: Detail,
    ) -> Result<(), KeyError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(by),
                action,
                target: Target::key(key),
                detail,
            })
            .await?)
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
        floor: OrgRole,
    ) -> Result<AccountId, KeyError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(KeyError::NotYours),
            Some(role) if !role.at_least(floor) => Err(KeyError::NotYoursToChange),
            Some(_) => Ok(who.account),
        }
    }
}
