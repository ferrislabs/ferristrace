use super::errors::ExploreError;
use super::ports::SpanReader;
use std::borrow::Cow;

use super::value_objects::Window;
use super::query::AttributeKey;
use super::value_objects::{
    AttributeFilter, SeenAttribute, SeenValue, ServiceFilter, ServiceSeries, ServiceSummary,
    TraceFilter, TraceSummary,
};
use crate::access::{ApiKeys, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::identity::SessionStore;
use crate::organisation::Memberships;
use crate::placement::{PlacementReader, Placements, Sighting};
use crate::telemetry::{Span, TenantId, TraceId};

#[derive(Debug)]
pub struct Explore<S, G, K, R> {
    readers: Readers<S, G, K>,
    reader: R,
}

impl<S, G, K, R> Explore<S, G, K, R>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    R: SpanReader,
{
    pub fn new(readers: Readers<S, G, K>, reader: R) -> Self {
        Self { readers, reader }
    }

    pub async fn services(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, ExploreError> {
        let filter = self.narrowed(who, org, filter).await?;
        Ok(self.reader.services(org, &filter).await?)
    }

    pub async fn operations(
        &self,
        who: &Reader,
        org: TenantId,
        service: &str,
        filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, ExploreError> {
        let filter = self.narrowed(who, org, filter).await?;
        Ok(self.reader.operations(org, service, &filter).await?)
    }

    pub async fn series(
        &self,
        who: &Reader,
        org: TenantId,
        service: Option<&str>,
        filter: &ServiceFilter,
    ) -> Result<ServiceSeries, ExploreError> {
        let filter = self.narrowed(who, org, filter).await?;
        Ok(self
            .reader
            .series(org, service, &filter)
            .await?
            .filled(&filter))
    }

    async fn narrowed<'f>(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &'f ServiceFilter,
    ) -> Result<Cow<'f, ServiceFilter>, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(filter.project)?;
        if project == filter.project {
            return Ok(Cow::Borrowed(filter));
        }
        let mut narrowed = filter.clone();
        narrowed.project = project;
        Ok(Cow::Owned(narrowed))
    }

    pub async fn traces(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(filter.project)?;
        if project == filter.project {
            return Ok(self.reader.traces(org, filter).await?);
        }
        let mut narrowed = filter.clone();
        narrowed.project = project;
        Ok(self.reader.traces(org, &narrowed).await?)
    }

    pub async fn attributes(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenAttribute>, ExploreError> {
        let filter = self.narrowed_attributes(who, org, filter).await?;
        Ok(self.reader.attributes(org, &filter).await?)
    }

    pub async fn attribute_values(
        &self,
        who: &Reader,
        org: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenValue>, ExploreError> {
        let filter = self.narrowed_attributes(who, org, filter).await?;
        Ok(self.reader.attribute_values(org, key, &filter).await?)
    }

    async fn narrowed_attributes(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &AttributeFilter,
    ) -> Result<AttributeFilter, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(filter.project)?;
        Ok(AttributeFilter {
            project,
            ..filter.clone()
        })
    }

    pub async fn trace(
        &self,
        who: &Reader,
        org: TenantId,
        id: TraceId,
    ) -> Result<Vec<Span>, ExploreError> {
        let grant = self.readers.grant(who, org).await?;
        Ok(self.reader.trace(org, id, grant.project).await?)
    }
}

impl<S, G, K, R> Explore<S, G, K, R>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    R: PlacementReader,
{
    pub async fn placements(
        &self,
        who: &Reader,
        org: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Placements, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(project)?;
        Ok(self.reader.placements(org, window, project).await?.capped())
    }

    pub async fn sighting(
        &self,
        who: &Reader,
        org: TenantId,
        service: &str,
        window: Window,
    ) -> Result<Sighting, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(None)?;
        Ok(self.reader.sighting(org, service, window, project).await?)
    }
}
