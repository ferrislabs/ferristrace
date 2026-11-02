use super::entities::MetricDescriptor;
use super::errors::MetricError;
use super::ports::MetricReader;
use super::value_objects::{MetricQuery, SeriesSet};
use crate::access::{ApiKeys, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::explore::ExploreError;
use crate::identity::SessionStore;
use crate::organisation::Memberships;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct MetricSearch<S, G, K, R> {
    readers: Readers<S, G, K>,
    reader: R,
}

impl<S, G, K, R> MetricSearch<S, G, K, R>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    R: MetricReader,
{
    pub fn new(readers: Readers<S, G, K>, reader: R) -> Self {
        Self { readers, reader }
    }

    pub async fn catalogue(
        &self,
        who: &Reader,
        org: TenantId,
        search: Option<&str>,
        project: Option<ProjectId>,
    ) -> Result<Vec<MetricDescriptor>, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(project)?;
        Ok(self.reader.catalogue(org, search, project).await?)
    }

    pub async fn series(
        &self,
        who: &Reader,
        org: TenantId,
        query: &MetricQuery,
    ) -> Result<SeriesSet, MetricError> {
        let project = self
            .readers
            .grant(who, org)
            .await
            .map_err(ExploreError::from)?
            .narrow(query.project)
            .map_err(ExploreError::from)?;
        let kind = self
            .reader
            .catalogue(org, Some(&query.name), project)
            .await?
            .into_iter()
            .find(|metric| metric.name == query.name)
            .ok_or(MetricError::Unknown)?
            .kind;
        let aggregation = query.resolved(kind)?;
        if project == query.project {
            return Ok(self.reader.series(org, kind, aggregation, query).await?);
        }
        let mut narrowed = query.clone();
        narrowed.project = project;
        Ok(self
            .reader
            .series(org, kind, aggregation, &narrowed)
            .await?)
    }
}
