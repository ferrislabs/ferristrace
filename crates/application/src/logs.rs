use ferristrace_domain::{
    Buckets, ExploreError, LogBucket, LogPage, LogQuery, LogSearch, Reader, Readers, TenantId,
    Whoami,
};

use crate::registry::ReadRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: ReadRepos,
{
    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn search_logs(
        &self,
        who: &Reader,
        org: TenantId,
        query: &LogQuery,
    ) -> Result<LogPage, ExploreError> {
        let Some(reader) = &self.log_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        LogSearch::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .search(who, org, query)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn log_histogram(
        &self,
        who: &Reader,
        org: TenantId,
        query: &LogQuery,
        buckets: Buckets,
    ) -> Result<Vec<LogBucket>, ExploreError> {
        let Some(reader) = &self.log_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        LogSearch::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .histogram(who, org, query, buckets)
        .await
    }
}
