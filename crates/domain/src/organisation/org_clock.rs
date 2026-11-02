use super::ports::{Memberships, Orgs};
use super::value_objects::OrgTimezone;
use crate::access::{ApiKeys, ReadRefusal, Reader, Readers};
use crate::identity::SessionStore;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct OrgClock<S, G, K, O> {
    readers: Readers<S, G, K>,
    orgs: O,
}

impl<S, G, K, O> OrgClock<S, G, K, O>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    O: Orgs,
{
    pub fn new(readers: Readers<S, G, K>, orgs: O) -> Self {
        Self { readers, orgs }
    }

    pub async fn timezone(&self, who: &Reader, org: TenantId) -> Result<OrgTimezone, ReadRefusal> {
        self.readers.grant(who, org).await?;
        let card = self.orgs.only(org).await?.ok_or(ReadRefusal::NotYours)?;
        Ok(card.timezone)
    }
}
