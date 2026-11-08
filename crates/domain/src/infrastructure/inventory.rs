use std::borrow::Cow;

use super::entities::Resource;
use super::ports::InfraReader;
use super::value_objects::InfraFilter;
use crate::access::{ApiKeys, Reader, Readers};
use crate::clock::UnixSeconds;
use crate::cluster::fresh_window;
use crate::explore::ExploreError;
use crate::identity::SessionStore;
use crate::organisation::Memberships;
use crate::telemetry::TenantId;

const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug)]
pub struct Inventory<S, G, K, R> {
    readers: Readers<S, G, K>,
    reader: R,
}

impl<S, G, K, R> Inventory<S, G, K, R>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    R: InfraReader,
{
    pub fn new(readers: Readers<S, G, K>, reader: R) -> Self {
        Self { readers, reader }
    }

    pub async fn list(
        &self,
        who: &Reader,
        org: TenantId,
        now: UnixSeconds,
        filter: &InfraFilter,
    ) -> Result<Vec<Resource>, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(filter.project)?;
        let filter = if project == filter.project {
            Cow::Borrowed(filter)
        } else {
            Cow::Owned(InfraFilter {
                project,
                ..filter.clone()
            })
        };
        let now_nanos = now.get().saturating_mul(NANOS_PER_SECOND);
        let kept = self
            .reader
            .resources(org, fresh_window(now), &filter)
            .await?
            .into_iter()
            .map(|resource| resource.settled(now_nanos))
            .filter(|resource| {
                filter
                    .status
                    .is_none_or(|status| status.holds(resource.silent))
            })
            .collect();
        Ok(kept)
    }
}
