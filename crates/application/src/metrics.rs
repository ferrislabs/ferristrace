use ferristrace_domain::{
    ExploreError, MetricDescriptor, MetricError, MetricQuery, MetricSearch, ProjectId, Reader,
    Readers, SeriesSet, TenantId, Whoami,
};

use crate::registry::ReadRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: ReadRepos,
{
    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn metric_catalogue(
        &self,
        who: &Reader,
        org: TenantId,
        search: Option<&str>,
        project: Option<ProjectId>,
    ) -> Result<Vec<MetricDescriptor>, ExploreError> {
        let Some(reader) = &self.metric_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        MetricSearch::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .catalogue(who, org, search, project)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = MetricError::StoreUnreachable)]
    pub async fn metric_series(
        &self,
        who: &Reader,
        org: TenantId,
        query: &MetricQuery,
    ) -> Result<SeriesSet, MetricError> {
        let Some(reader) = &self.metric_store else {
            return Err(MetricError::StoreUnreachable);
        };
        MetricSearch::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .series(who, org, query)
        .await
    }
}
