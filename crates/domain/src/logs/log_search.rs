use super::ports::LogReader;
use super::value_objects::{Buckets, LogBucket, LogPage, LogQuery};
use crate::access::{ApiKeys, Reader, Readers};
use crate::explore::ExploreError;
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
}
