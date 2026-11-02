use ferristrace_domain::{
    Consumption, ExploreError, Metering, SessionToken, TenantId, Whoami, Window,
};

use crate::registry::UsageRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: UsageRepos,
{
    #[transactional(sessions, memberships, usage_history, unavailable = ExploreError::StoreUnreachable)]
    pub async fn consumption(
        &self,
        token: &SessionToken,
        org: TenantId,
        window: Window,
    ) -> Result<Consumption, ExploreError> {
        Metering::new(
            Whoami::new(sessions, memberships),
            &self.meter,
            usage_history,
        )
        .consumption(token, org, window)
        .await
    }
}
