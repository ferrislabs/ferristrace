use super::errors::FoundError;
use super::ports::ControlPlane;
use super::provisioning_service::Provisioning;
use super::value_objects::Founded;
use crate::audit::AuditTrail;
use crate::identity::{SessionStore, SessionSwitch};
use crate::organisation::OrgName;
use crate::secret::SessionToken;

#[derive(Debug)]
pub struct Founding<S, C, A> {
    sessions: S,
    provisioning: Provisioning<C, A>,
}

impl<S, C, A> Founding<S, C, A>
where
    S: SessionStore + SessionSwitch,
    C: ControlPlane,
    A: AuditTrail,
{
    pub fn new(sessions: S, control_plane: C, trail: A) -> Self {
        Self {
            sessions,
            provisioning: Provisioning::new(control_plane, trail),
        }
    }

    pub async fn found(&self, token: &SessionToken, name: &OrgName) -> Result<Founded, FoundError> {
        let session = self.sessions.resolve(token).await?;
        let founded = self
            .provisioning
            .found_for(session.account, name, &session.email)
            .await?;
        self.sessions.switch(token, founded.org).await?;
        Ok(founded)
    }
}
