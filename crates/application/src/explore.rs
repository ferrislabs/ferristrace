use ferristrace_domain::{
    AttributeFilter, AttributeKey, Clock, Explore, ExploreError, Placements, ProjectId, Reader,
    Readers, SeenAttribute, SeenValue, ServiceFilter, ServiceSeries, ServiceSite, ServiceSummary,
    Span, TenantId, TraceFilter, TraceId, TraceSummary, Whoami, Window,
};

use crate::registry::{ReadRepos, SiteRepos};

const SIGHTING_SECS: u64 = 86_400;
const NANOS_PER_SECOND: u64 = 1_000_000_000;
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
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .services(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn operations(
        &self,
        who: &Reader,
        org: TenantId,
        service: &str,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .operations(who, org, service, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn service_series(
        &self,
        who: &Reader,
        org: TenantId,
        service: Option<&str>,
        filter: &ServiceFilter,
    ) -> Result<ServiceSeries, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .series(who, org, service, filter)
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
    pub async fn attributes(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenAttribute>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .attributes(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn attribute_values(
        &self,
        who: &Reader,
        org: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenValue>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .attribute_values(who, org, key, filter)
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

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn placements(
        &self,
        who: &Reader,
        org: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Placements, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .placements(who, org, window, project)
        .await
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: SiteRepos,
{
    #[transactional(sessions, memberships, api_keys, projects, unavailable = ExploreError::StoreUnreachable)]
    pub async fn service_site(
        &self,
        who: &Reader,
        org: TenantId,
        service: &str,
    ) -> Result<ServiceSite, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        let to = self.clock.now().get().saturating_mul(NANOS_PER_SECOND);
        let from = to.saturating_sub(SIGHTING_SECS * NANOS_PER_SECOND);
        let window =
            Window::new(from, to.max(from + 1)).map_err(|_| ExploreError::StoreUnreachable)?;
        let seen = Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .sighting(who, org, service, window)
        .await?;
        let held = ferristrace_domain::Projects::of(&projects, org).await?;
        Ok(ServiceSite::named(seen, &held))
    }
}
