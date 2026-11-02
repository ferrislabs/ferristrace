use super::errors::ExploreError;
use super::ports::SpanReader;
use super::value_objects::{ServiceSummary, TraceFilter, TraceSummary, Window};
use crate::access::{ApiKeys, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::identity::SessionStore;
use crate::organisation::Memberships;
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
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(project)?;
        Ok(self.reader.services(org, window, project).await?)
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
