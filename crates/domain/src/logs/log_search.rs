use super::ports::LogReader;
use super::value_objects::{Buckets, LogBucket, LogPage, LogQuery};
use crate::access::{ApiKeys, Reader, Readers};
use crate::explore::{
    AttributeFilter, AttributeKey, ExploreError, FacetFilter, Facets, SeenAttribute, SeenValue,
};
use crate::identity::SessionStore;
use crate::organisation::Memberships;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct LogSearch<S, G, K, R> {
    readers: Readers<S, G, K>,
    reader: R,
}

impl<S, G, K, R> LogSearch<S, G, K, R>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    R: LogReader,
{
    pub fn new(readers: Readers<S, G, K>, reader: R) -> Self {
        Self { readers, reader }
    }

    pub async fn search(
        &self,
        who: &Reader,
        org: TenantId,
        query: &LogQuery,
    ) -> Result<LogPage, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(query.project)?;
        if project == query.project {
            return Ok(self.reader.search(org, query).await?);
        }
        let mut narrowed = query.clone();
        narrowed.project = project;
        Ok(self.reader.search(org, &narrowed).await?)
    }

    pub async fn histogram(
        &self,
        who: &Reader,
        org: TenantId,
        query: &LogQuery,
        buckets: Buckets,
    ) -> Result<Vec<LogBucket>, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(query.project)?;
        if project == query.project {
            return Ok(self.reader.histogram(org, query, buckets).await?);
        }
        let mut narrowed = query.clone();
        narrowed.project = project;
        Ok(self.reader.histogram(org, &narrowed, buckets).await?)
    }

    pub async fn facets(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &FacetFilter,
    ) -> Result<Facets, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(filter.project)?;
        let mut narrowed = filter.clone();
        narrowed.project = project;
        Ok(self.reader.facets(org, &narrowed).await?)
    }

    pub async fn attributes(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenAttribute>, ExploreError> {
        let filter = self.narrowed(who, org, filter).await?;
        Ok(self.reader.attributes(org, &filter).await?)
    }

    pub async fn attribute_values(
        &self,
        who: &Reader,
        org: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> Result<Vec<SeenValue>, ExploreError> {
        let filter = self.narrowed(who, org, filter).await?;
        Ok(self.reader.attribute_values(org, key, &filter).await?)
    }

    async fn narrowed(
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
}
