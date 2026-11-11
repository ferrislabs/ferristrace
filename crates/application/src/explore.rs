use ferristrace_domain::{
    AttributeFilter, AttributeKey, CacheGrouping, CacheSeries, CacheSummary, Clock, Explore,
    ExploreError, FacetFilter, Facets, Fanout, JobGrouping, JobSeries, JobSummary, ListedSpan,
    Placements, ProjectId, Reader, Readers, RequestGrouping, RequestSeries, RequestSummary,
    SeenAttribute, SeenValue, SeriesFilter, SeriesResult, ServiceEdge, ServiceFilter,
    ServiceSeries, ServiceSite, ServiceSummary, Span, SpanFilter, StatementGrouping,
    StatementSeries,
    StatementSummary, TableFilter, TableRow, TenantId, TraceFilter, TraceId, TraceSummary, Whoami,
    Window,
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
    pub async fn dependencies(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceEdge>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .dependencies(who, org, filter)
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
    pub async fn caches(
        &self,
        who: &Reader,
        org: TenantId,
        grouping: CacheGrouping,
        filter: &ServiceFilter,
    ) -> Result<Vec<CacheSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .caches(who, org, grouping, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn cache_series(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ServiceFilter,
    ) -> Result<CacheSeries, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .cache_series(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn requests(
        &self,
        who: &Reader,
        org: TenantId,
        grouping: RequestGrouping,
        filter: &ServiceFilter,
    ) -> Result<Vec<RequestSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .requests(who, org, grouping, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn request_series(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ServiceFilter,
    ) -> Result<RequestSeries, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .request_series(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn jobs(
        &self,
        who: &Reader,
        org: TenantId,
        grouping: JobGrouping,
        filter: &ServiceFilter,
    ) -> Result<Vec<JobSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .jobs(who, org, grouping, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn job_series(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ServiceFilter,
    ) -> Result<JobSeries, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .job_series(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn statements(
        &self,
        who: &Reader,
        org: TenantId,
        grouping: StatementGrouping,
        filter: &ServiceFilter,
    ) -> Result<Vec<StatementSummary>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .statements(who, org, grouping, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn statement_series(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ServiceFilter,
    ) -> Result<StatementSeries, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .statement_series(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn statement_fanouts(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ServiceFilter,
    ) -> Result<Vec<Fanout>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .statement_fanouts(who, org, filter)
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
    pub async fn spans(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &SpanFilter,
    ) -> Result<Vec<ListedSpan>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .spans(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn span_series(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &SeriesFilter,
    ) -> Result<SeriesResult, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .span_series(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn span_table(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &TableFilter,
    ) -> Result<Vec<TableRow>, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .span_table(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, unavailable = ExploreError::StoreUnreachable)]
    pub async fn facets(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &FacetFilter,
    ) -> Result<Facets, ExploreError> {
        let Some(reader) = &self.span_store else {
            return Err(ExploreError::StoreUnreachable);
        };
        Explore::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
        )
        .facets(who, org, filter)
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
