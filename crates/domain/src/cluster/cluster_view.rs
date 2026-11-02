use std::borrow::Cow;

use super::entities::ClusterSnapshot;
use super::ports::ClusterReader;
use super::value_objects::{ClusterFilter, FRESH_SECS};
use crate::access::{ApiKeys, Reader, Readers};
use crate::clock::UnixSeconds;
use crate::explore::{ExploreError, Window};
use crate::identity::SessionStore;
use crate::organisation::Memberships;
use crate::telemetry::TenantId;

const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug)]
pub struct ClusterView<S, G, K, R> {
    readers: Readers<S, G, K>,
    reader: R,
}

pub fn fresh_window(now: UnixSeconds) -> Window {
    let to = now.get().saturating_add(1).saturating_mul(NANOS_PER_SECOND);
    let from = to.saturating_sub((FRESH_SECS + 1) * NANOS_PER_SECOND);
    Window::new(from, to).expect("a fresh window ends at least a second after it starts")
}

impl<S, G, K, R> ClusterView<S, G, K, R>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    R: ClusterReader,
{
    pub fn new(readers: Readers<S, G, K>, reader: R) -> Self {
        Self { readers, reader }
    }

    pub async fn snapshot(
        &self,
        who: &Reader,
        org: TenantId,
        now: UnixSeconds,
        filter: &ClusterFilter,
    ) -> Result<ClusterSnapshot, ExploreError> {
        let project = self.readers.grant(who, org).await?.narrow(filter.project)?;
        let filter = if project == filter.project {
            Cow::Borrowed(filter)
        } else {
            Cow::Owned(ClusterFilter {
                project,
                ..filter.clone()
            })
        };
        Ok(self
            .reader
            .snapshot(org, fresh_window(now), &filter)
            .await?
            .settled(&filter))
    }
}
