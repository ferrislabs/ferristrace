use ferristrace_domain::{
    FoundError, Founded, Founding, OrgName, SessionToken, SwitchError, Switching, TenantId,
};

use crate::registry::{FoundRepos, SwitchRepos};
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: SwitchRepos,
{
    #[transactional(sessions, memberships, unavailable = SwitchError::StoreUnreachable)]
    pub async fn switch_org(&self, token: &SessionToken, org: TenantId) -> Result<(), SwitchError> {
        Switching::new(sessions, memberships).to(token, org).await
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: FoundRepos,
{
    #[transactional(sessions, control_plane, audit, unavailable = FoundError::Unreachable)]
    pub async fn found_org(
        &self,
        token: &SessionToken,
        name: &OrgName,
    ) -> Result<Founded, FoundError> {
        Founding::new(sessions, control_plane, audit)
            .found(token, name)
            .await
    }
}
