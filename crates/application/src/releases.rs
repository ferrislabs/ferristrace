use core::future::Future;

use ferristrace_domain::{
    Clock, Deploy, DeployFeed, Followed, ProjectId, Reader, Readers, Release, ReleaseDetail,
    ReleaseError, ReleaseFilter, Releases, TenantId, Watch, Whoami,
};

use crate::registry::{DeployRepos, ReadRepos};
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: ReadRepos,
{
    #[transactional(sessions, memberships, api_keys, unavailable = ReleaseError::StoreUnreachable)]
    pub async fn releases(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Release>, ReleaseError> {
        let Some(reader) = &self.issue_store else {
            return Err(ReleaseError::StoreUnreachable);
        };
        Releases::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .list(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ReleaseError::StoreUnreachable)]
    pub async fn release(
        &self,
        who: &Reader,
        org: TenantId,
        service: &str,
        version: &str,
        project: Option<ProjectId>,
    ) -> Result<ReleaseDetail, ReleaseError> {
        let Some(reader) = &self.issue_store else {
            return Err(ReleaseError::StoreUnreachable);
        };
        Releases::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .detail(who, org, service, version, project)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ReleaseError::StoreUnreachable)]
    pub async fn deploys(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Deploy>, ReleaseError> {
        let Some(reader) = &self.issue_store else {
            return Err(ReleaseError::StoreUnreachable);
        };
        Releases::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .deploys(who, org, filter)
        .await
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: DeployRepos,
    S::IssueStore: DeployFeed,
{
    #[allow(clippy::manual_async_fn)]
    pub fn follow_deploys(&self) -> impl Future<Output = Result<Followed, ReleaseError>> + Send {
        async move { self.follow_once().await }
    }

    #[transactional(incidents, incident_timeline, alert_rules, deploy_marks, alert_duty, unavailable = ReleaseError::StoreUnreachable)]
    async fn follow_once(&self) -> Result<Followed, ReleaseError> {
        let Some(feed) = &self.issue_store else {
            return Err(ReleaseError::StoreUnreachable);
        };
        Watch::new(
            feed,
            incidents,
            incident_timeline,
            alert_rules,
            deploy_marks,
            alert_duty,
        )
        .tick(self.clock.now())
        .await
    }
}
