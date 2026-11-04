use ferristrace_domain::{
    Session, SessionError, SessionStore, SessionToken, UserRefusal, UserTokens,
};

use crate::registry::StandingRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S> {
    pub async fn verify_person(&self, token: &str) -> Result<SessionToken, UserRefusal> {
        let Some(users) = &self.users else {
            return Err(UserRefusal::Off);
        };
        users.verify(token).await
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: StandingRepos,
{
    #[transactional(sessions, unavailable = SessionError::StoreUnreachable)]
    pub async fn resolve_session(&self, token: &SessionToken) -> Result<Session, SessionError> {
        sessions.resolve(token).await
    }
}
