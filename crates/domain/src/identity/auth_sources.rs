use core::fmt;

use super::errors::AuthError;
use super::ports::AuthProvider;
use super::value_objects::{ResolvedIdentity, SignInProof};

pub struct AuthSources<L, F> {
    local: Option<L>,
    federated: Option<F>,
}

impl<L, F> AuthSources<L, F>
where
    L: AuthProvider,
    F: AuthProvider,
{
    pub fn new(local: Option<L>, federated: Option<F>) -> Self {
        Self { local, federated }
    }

    pub fn local_enabled(&self) -> bool {
        self.local.is_some()
    }

    pub fn federated_enabled(&self) -> bool {
        self.federated.is_some()
    }

    pub async fn resolve(&self, proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        match proof {
            SignInProof::Password { .. } => {
                let local = self.local.as_ref().ok_or(AuthError::SourceDisabled)?;
                local.sign_in(proof).await
            }
            SignInProof::Federated { .. } => {
                let federated = self.federated.as_ref().ok_or(AuthError::SourceDisabled)?;
                federated.sign_in(proof).await
            }
        }
    }
}

impl<L, F> fmt::Debug for AuthSources<L, F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthSources")
            .field("local", &self.local.is_some())
            .field("federated", &self.federated.is_some())
            .finish()
    }
}
