use ferristrace_domain::{
    ApiKey, IssuedKey, KeyError, KeyId, KeyName, KeyRing, KeyScopes, ProjectId, SessionToken,
    TenantId, Whoami,
};

use crate::registry::AccessRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: AccessRepos,
{
    #[transactional(sessions, memberships, api_keys, projects, unavailable = KeyError::StoreUnreachable)]
    pub async fn list_keys(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<ApiKey>, KeyError> {
        KeyRing::new(Whoami::new(sessions, memberships), api_keys, projects)
            .list(token, org)
            .await
    }

    #[transactional(sessions, memberships, api_keys, projects, unavailable = KeyError::StoreUnreachable)]
    pub async fn issue_key(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
    ) -> Result<IssuedKey, KeyError> {
        KeyRing::new(Whoami::new(sessions, memberships), api_keys, projects)
            .issue(token, org, project, name, scopes)
            .await
    }

    #[transactional(sessions, memberships, api_keys, projects, unavailable = KeyError::StoreUnreachable)]
    pub async fn revoke_key(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: KeyId,
    ) -> Result<(), KeyError> {
        KeyRing::new(Whoami::new(sessions, memberships), api_keys, projects)
            .revoke(token, org, id)
            .await
    }
}
