use super::auth_sources::AuthSources;
use super::entities::Session;
use super::errors::SignInError;
use super::ports::{AccountDirectory, AuthProvider, Gate, SessionStore};
use super::value_objects::{Agent, ChallengeToken, Opened, ProvisionPolicy, SignInProof};
use crate::secret::SessionToken;

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
