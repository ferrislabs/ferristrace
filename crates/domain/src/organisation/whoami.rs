use super::errors::WhoamiError;
use super::ports::Memberships;
use super::value_objects::Who;
use crate::identity::SessionStore;
use crate::secret::SessionToken;

#[derive(Debug)]
pub struct Whoami<S, G> {
    sessions: S,
    memberships: G,
}

impl<S, G> Whoami<S, G>
where
    S: SessionStore,
    G: Memberships,
{
    pub fn new(sessions: S, memberships: G) -> Self {
        Self {
            sessions,
            memberships,
        }
    }

    pub async fn of(&self, token: &SessionToken) -> Result<Who, WhoamiError> {
        let session = self.sessions.resolve(token).await?;
        let memberships = self.memberships.of(session.account).await?;
        Ok(Who {
            account: session.account,
            email: session.email,
            name: session.name,
            photo: session.photo,
            active_org: session.org,
            memberships,
        })
    }
}
