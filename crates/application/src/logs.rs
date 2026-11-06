use ferristrace_domain::{
    AttributeFilter, AttributeKey, Buckets, ExploreError, FacetFilter, Facets, LogBucket, LogPage,
    LogQuery, LogSearch, Reader, Readers, SeenAttribute, SeenValue, TenantId, Whoami,
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

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn log_facets(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &FacetFilter,
    ) -> Result<Facets, ExploreError> {
        let Some(reader) = &self.log_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        LogSearch::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .facets(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn log_attributes(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenAttribute>, ExploreError> {
        let Some(reader) = &self.log_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        LogSearch::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .attributes(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn log_attribute_values(
        &self,
        who: &Reader,
        org: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenValue>, ExploreError> {
        let Some(reader) = &self.log_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        LogSearch::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .attribute_values(who, org, key, filter)
        .await
    }
}
