use super::errors::ReadRefusal;
use super::ports::ApiKeys;
use super::value_objects::{ReadGrant, Reader};
use crate::identity::SessionStore;
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Readers<S, G, K> {
    whoami: Whoami<S, G>,
    keys: K,
}

impl<S, G, K> Readers<S, G, K>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
{
    pub fn new(whoami: Whoami<S, G>, keys: K) -> Self {
        Self { whoami, keys }
    }

    pub fn whoami(&self) -> &Whoami<S, G> {
        &self.whoami
    }

    pub async fn grant(&self, reader: &Reader, org: TenantId) -> Result<ReadGrant, ReadRefusal> {
        match reader {
            Reader::Session(token) => {
                let who = self.whoami.of(token).await?;
                if who.may_act_in(org, OrgRole::Member) {
                    Ok(ReadGrant::whole(org))
                } else {
                    Err(ReadRefusal::NotYours)
                }
            }
            Reader::Key(secret) => {
                let grant = self
                    .keys
                    .grant_for(secret)
                    .await?
                    .ok_or(ReadRefusal::Unknown)?;
                if grant.org == org {
                    Ok(grant)
                } else {
                    Err(ReadRefusal::NotYours)
                }
            }
        }
    }
}
