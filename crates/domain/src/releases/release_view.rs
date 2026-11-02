use std::borrow::Cow;

use super::entities::{Deploy, Release, ReleaseDetail};
use super::errors::ReleaseError;
use super::ports::ReleaseReader;
use super::value_objects::ReleaseFilter;
use crate::access::{ApiKeys, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::identity::SessionStore;
use crate::organisation::Memberships;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Releases<S, G, K, R> {
    readers: Readers<S, G, K>,
    reader: R,
}

impl<S, G, K, R> Releases<S, G, K, R>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    R: ReleaseReader,
{
    pub fn new(readers: Readers<S, G, K>, reader: R) -> Self {
        Self { readers, reader }
    }

    pub async fn list(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Release>, ReleaseError> {
        let filter = self.narrowed(who, org, filter).await?;
        Ok(self.reader.releases(org, &filter).await?)
    }

    pub async fn deploys(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Deploy>, ReleaseError> {
        let filter = self.narrowed(who, org, filter).await?;
        Ok(self.reader.deploys(org, &filter).await?)
    }

    pub async fn detail(
        &self,
        who: &Reader,
        org: TenantId,
        service: &str,
        version: &str,
        project: Option<ProjectId>,
    ) -> Result<ReleaseDetail, ReleaseError> {
        let project = self.readers.grant(who, org).await?.narrow(project)?;
        self.reader
            .release(org, service, version, project)
            .await?
            .ok_or(ReleaseError::Unknown)
    }

    async fn narrowed<'f>(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &'f ReleaseFilter,
    ) -> Result<Cow<'f, ReleaseFilter>, ReleaseError> {
        let project = self.readers.grant(who, org).await?.narrow(filter.project)?;
        if project == filter.project {
            return Ok(Cow::Borrowed(filter));
        }
        let mut narrowed = filter.clone();
        narrowed.project = project;
        Ok(Cow::Owned(narrowed))
    }
}
