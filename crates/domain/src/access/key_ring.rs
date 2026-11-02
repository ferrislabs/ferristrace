use super::entities::{ApiKey, IssuedKey};
use super::errors::KeyError;
use super::ports::ApiKeys;
use super::value_objects::{KeyId, KeyName, KeyScopes};
use crate::catalogue::{ProjectId, Projects};
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct KeyRing<S, G, K, P> {
    whoami: Whoami<S, G>,
    keys: K,
    projects: P,
}

impl<S, G, K, P> KeyRing<S, G, K, P>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    P: Projects,
{
    pub fn new(whoami: Whoami<S, G>, keys: K, projects: P) -> Self {
        Self {
            whoami,
            keys,
            projects,
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
    ) -> Result<IssuedKey, KeyError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        if let Some(project) = project {
            let held = self.projects.of(org).await?;
            if !held.iter().any(|kept| kept.id == project) {
                return Err(KeyError::NoSuchProject);
            }
        }
        Ok(self.keys.issue(org, project, name, scopes, by).await?)
    }

    pub async fn revoke(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: KeyId,
    ) -> Result<(), KeyError> {
        self.standing(token, org, OrgRole::Admin).await?;
        if self.keys.revoke(org, id).await? {
            Ok(())
        } else {
            Err(KeyError::NoSuchKey)
        }
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
