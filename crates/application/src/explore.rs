use ferristrace_domain::{
    Explore, ExploreError, ProjectId, Reader, Readers, ServiceSummary, Span, TenantId, TraceFilter,
    TraceId, TraceSummary, Whoami, Window,
};

use crate::registry::ReadRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: ReadRepos,
{
    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn services(
        &self,
        who: &Reader,
        org: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .services(who, org, window, project)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn traces(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .traces(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn trace(
        &self,
        who: &Reader,
        org: TenantId,
        id: TraceId,
    ) -> Result<Vec<Span>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .trace(who, org, id)
        .await
    }
}
