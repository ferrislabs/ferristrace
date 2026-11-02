use super::ports::{UsageHistory, UsageReader};
use super::value_objects::{Consumption, DecompressedBytes, TenantId};
use crate::explore::{ExploreError, Window};
use crate::identity::SessionStore;
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::secret::SessionToken;

#[derive(Debug)]
pub struct Metering<S, G, R, H> {
    whoami: Whoami<S, G>,
    live: R,
    history: H,
}

impl<S, G, R, H> Metering<S, G, R, H>
where
    S: SessionStore,
    G: Memberships,
    R: UsageReader,
    H: UsageHistory,
{
    pub fn new(whoami: Whoami<S, G>, live: R, history: H) -> Self {
        Self {
            whoami,
            live,
            history,
        }
    }

    pub async fn consumption(
        &self,
        token: &SessionToken,
        org: TenantId,
        window: Window,
    ) -> Result<Consumption, ExploreError> {
        let who = self.whoami.of(token).await?;
        if !who.may_act_in(org, OrgRole::Member) {
            return Err(ExploreError::NotYours);
        }
        Ok(Consumption {
            ingested_bytes: self.live.used(org).await.get(),
            included_bytes: self
                .history
                .included(org)
                .await?
                .map(DecompressedBytes::get),
            hours: self.history.hours(org, window).await?,
        })
    }
}
