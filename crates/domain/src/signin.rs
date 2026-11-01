use core::future::Future;

use crate::identity::{
    AccountId, AuthError, AuthProvider, AuthSources, ProvisionPolicy, ResolvedIdentity, SignInProof,
};
use crate::second_factor::{ChallengeToken, FactorError, FactorKind, Gate};
use crate::secret::SessionToken;
use crate::session::{Agent, Session, SessionError, SessionStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignInError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Factor(#[from] FactorError),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    Session(SessionToken),
    SecondStep {
        challenge: ChallengeToken,
        offered: Vec<FactorKind>,
    },
}

pub trait AccountDirectory {
    fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> impl Future<Output = Result<AccountId, AuthError>> + Send;
}

impl<T: AccountDirectory + Sync> AccountDirectory for &T {
    fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> impl Future<Output = Result<AccountId, AuthError>> + Send {
        (*self).provision(identity, policy)
    }
}

#[derive(Debug)]
pub struct SignIn<L, F, A, S, G> {
    sources: AuthSources<L, F>,
    accounts: A,
    sessions: S,
    gate: G,
    policy: ProvisionPolicy,
}

impl<L, F, A, S, G> SignIn<L, F, A, S, G>
where
    L: AuthProvider,
    F: AuthProvider,
    A: AccountDirectory,
    S: SessionStore,
    G: Gate,
{
    pub fn new(
        sources: AuthSources<L, F>,
        accounts: A,
        sessions: S,
        gate: G,
        policy: ProvisionPolicy,
    ) -> Self {
        Self {
            sources,
            accounts,
            sessions,
            gate,
            policy,
        }
    }

    pub fn local_enabled(&self) -> bool {
        self.sources.local_enabled()
    }

    pub fn federated_enabled(&self) -> bool {
        self.sources.federated_enabled()
    }

    pub async fn begin(
        &self,
        proof: SignInProof,
        from: Option<&Agent>,
    ) -> Result<Opened, SignInError> {
        let identity = self.sources.resolve(proof).await?;
        let account = self.accounts.provision(&identity, self.policy).await?;
        let offered = self.gate.offered(account).await?;
        if offered.is_empty() {
            let token = self.sessions.issue(account, None, from).await?;
            return Ok(Opened::Session(token));
        }
        let challenge = ChallengeToken::random();
        self.gate.open(account, challenge.hash()).await?;
        Ok(Opened::SecondStep { challenge, offered })
    }

    pub async fn resolve(&self, token: &SessionToken) -> Result<Session, SignInError> {
        Ok(self.sessions.resolve(token).await?)
    }

    pub async fn end(&self, token: &SessionToken) -> Result<(), SignInError> {
        Ok(self.sessions.revoke(token).await?)
    }
}

impl<T: AccountDirectory + Send + Sync> AccountDirectory for std::sync::Arc<T> {
    fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> impl Future<Output = Result<AccountId, AuthError>> + Send {
        self.as_ref().provision(identity, policy)
    }
}
