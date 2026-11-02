use ferristrace_domain::{
    Agent, AuthError, FederatedError, FederatedSignIn, FlowError, LiveSession, RollError, Session,
    SessionError, SessionId, SessionStore, SessionToken, Sessions, SignInError,
};

use crate::borrowed::Borrowed;
use crate::registry::IdentityRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: IdentityRepos,
{
    #[transactional(
        sessions,
        unavailable = SignInError::Session(SessionError::StoreUnreachable)
    )]
    pub async fn resolve_session(&self, token: &SessionToken) -> Result<Session, SignInError> {
        Ok(sessions.resolve(token).await?)
    }

    #[transactional(
        sessions,
        unavailable = SignInError::Session(SessionError::StoreUnreachable)
    )]
    pub async fn sign_out(&self, token: &SessionToken) -> Result<(), SignInError> {
        Ok(sessions.revoke(token).await?)
    }

    #[transactional(sessions, unavailable = RollError::Unavailable)]
    pub async fn live_sessions(&self, token: &SessionToken) -> Result<Vec<LiveSession>, RollError> {
        Sessions::new(&sessions, Borrowed(&sessions))
            .live(token)
            .await
    }

    #[transactional(sessions, unavailable = RollError::Unavailable)]
    pub async fn end_session(&self, token: &SessionToken, id: SessionId) -> Result<(), RollError> {
        Sessions::new(&sessions, Borrowed(&sessions))
            .end_one(token, id)
            .await
    }

    #[transactional(sessions, unavailable = RollError::Unavailable)]
    pub async fn end_other_sessions(&self, token: &SessionToken) -> Result<u64, RollError> {
        Sessions::new(&sessions, Borrowed(&sessions))
            .end_others(token)
            .await
    }

    #[transactional(flows, accounts, sessions, unavailable = FederatedError::Flow(FlowError::StoreUnreachable))]
    pub async fn start_federated_sign_in(&self) -> Result<String, FederatedError> {
        let Some(provider) = &self.oidc else {
            return Err(FederatedError::Auth(AuthError::SourceDisabled));
        };
        FederatedSignIn::new(provider, flows, accounts, sessions, self.provision)
            .start()
            .await
    }

    #[transactional(
        flows,
        accounts,
        sessions,
        unavailable = FederatedError::Flow(FlowError::StoreUnreachable),
        settle = always
    )]
    pub async fn finish_federated_sign_in(
        &self,
        code: String,
        state: &str,
        from: Option<&Agent>,
    ) -> Result<SessionToken, FederatedError> {
        let Some(provider) = &self.oidc else {
            return Err(FederatedError::Auth(AuthError::SourceDisabled));
        };
        FederatedSignIn::new(provider, flows, accounts, sessions, self.provision)
            .finish(code, state, from)
            .await
    }
}
