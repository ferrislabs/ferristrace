use super::errors::SwitchError;
use super::ports::Memberships;
use crate::identity::{SessionStore, SessionSwitch};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Switching<S, G> {
    sessions: S,
    memberships: G,
}

impl<S, G> Switching<S, G>
where
    S: SessionStore + SessionSwitch,
    G: Memberships,
{
    pub fn new(sessions: S, memberships: G) -> Self {
        Self {
            sessions,
            memberships,
        }
    }

    pub async fn to(&self, token: &SessionToken, org: TenantId) -> Result<(), SwitchError> {
        let session = self.sessions.resolve(token).await?;
        let memberships = self.memberships.of(session.account).await?;
        if !memberships.iter().any(|membership| membership.org == org) {
            return Err(SwitchError::NotYours);
        }
        Ok(self.sessions.switch(token, org).await?)
    }
}
