use ferristrace_domain::{
    Explore, ExploreError, ProjectId, ServiceSummary, SessionToken, Span, TenantId, TraceFilter,
    TraceId, TraceSummary, Whoami, Window,
};

use crate::registry::StandingRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: StandingRepos,
{
    #[transactional(sessions, memberships, unavailable = ExploreError::StoreUnreachable)]
    pub async fn services(
        &self,
        token: &SessionToken,
        org: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(Whoami::new(sessions, memberships), reader)
            .services(token, org, window, project)
            .await
    }

    #[transactional(sessions, memberships, unavailable = ExploreError::StoreUnreachable)]
    pub async fn traces(
        &self,
        token: &SessionToken,
        org: TenantId,
        filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(Whoami::new(sessions, memberships), reader)
            .traces(token, org, filter)
            .await
    }

    #[transactional(sessions, memberships, unavailable = ExploreError::StoreUnreachable)]
    pub async fn trace(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: TraceId,
    ) -> Result<Vec<Span>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(Whoami::new(sessions, memberships), reader)
            .trace(token, org, id)
            .await
    }
}
