use super::errors::RollError;
use super::ports::{SessionRoll, SessionStore};
use super::value_objects::{LiveSession, SessionId};
use crate::secret::SessionToken;

#[derive(Debug)]
pub struct Sessions<S, R> {
    sessions: S,
    roll: R,
}

impl<S, R> Sessions<S, R>
where
    S: SessionStore,
    R: SessionRoll,
{
    pub fn new(sessions: S, roll: R) -> Self {
        Self { sessions, roll }
    }

    pub async fn live(&self, token: &SessionToken) -> Result<Vec<LiveSession>, RollError> {
        let session = self.sessions.resolve(token).await?;
        Ok(self.roll.live(session.account, token).await?)
    }

    pub async fn end_one(&self, token: &SessionToken, id: SessionId) -> Result<(), RollError> {
        let session = self.sessions.resolve(token).await?;
        if self.roll.revoke_one(session.account, id).await? {
            return Ok(());
        }
        Err(RollError::NoSuchSession)
    }

    pub async fn end_others(&self, token: &SessionToken) -> Result<u64, RollError> {
        let session = self.sessions.resolve(token).await?;
        Ok(self.roll.revoke_others(session.account, token).await?)
    }
}
