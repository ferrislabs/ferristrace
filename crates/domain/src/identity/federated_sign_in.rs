use super::errors::FederatedError;
use super::ports::{AccountDirectory, AuthProvider, Authorizer, OidcFlows, SessionStore};
use super::value_objects::{Agent, ProvisionPolicy, SignInProof};
use crate::secret::SessionToken;

#[derive(Debug)]
pub struct FederatedSignIn<Z, W, A, S> {
    provider: Z,
    flows: W,
    accounts: A,
    sessions: S,
    policy: ProvisionPolicy,
}

impl<Z, W, A, S> FederatedSignIn<Z, W, A, S>
where
    Z: Authorizer + AuthProvider,
    W: OidcFlows,
    A: AccountDirectory,
    S: SessionStore,
{
    pub fn new(provider: Z, flows: W, accounts: A, sessions: S, policy: ProvisionPolicy) -> Self {
        Self {
            provider,
            flows,
            accounts,
            sessions,
            policy,
        }
    }

    pub async fn start(&self) -> Result<String, FederatedError> {
        let (url, flow) = self.provider.begin()?;
        self.flows.begin(&flow).await?;
        Ok(url)
    }

    pub async fn finish(
        &self,
        code: String,
        state: &str,
        from: Option<&Agent>,
    ) -> Result<SessionToken, FederatedError> {
        let flow = self.flows.take(state).await?;
        let identity = self
            .provider
            .sign_in(SignInProof {
                code,
                code_verifier: flow.verifier.exposed().to_owned(),
                nonce: flow.nonce.clone(),
                redirect_uri: flow.redirect_uri.clone(),
            })
            .await?;
        let account = self.accounts.provision(&identity, self.policy).await?;
        Ok(self.sessions.issue(account, None, from).await?)
    }
}
