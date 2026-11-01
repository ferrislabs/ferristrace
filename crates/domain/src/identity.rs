use core::fmt;
use core::future::Future;

use uuid::Uuid;

use crate::credential::{Email, Issuer, Password, Subject};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("this way of signing in is not enabled")]
    SourceDisabled,
    #[error("the credentials match no account")]
    Rejected,
    #[error("the identity provider could not be reached")]
    ProviderUnreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountId(Uuid);

impl AccountId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum SignInProof {
    Password {
        email: Email,
        password: Password,
    },
    Federated {
        code: String,
        code_verifier: String,
        nonce: String,
        redirect_uri: String,
    },
}

impl fmt::Debug for SignInProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password { .. } => formatter.write_str("SignInProof::Password(redacted)"),
            Self::Federated { .. } => formatter.write_str("SignInProof::Federated(redacted)"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIdentity {
    pub issuer: Issuer,
    pub subject: Subject,
    pub email: Email,
    pub email_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProvisionPolicy {
    pub auto_provision: bool,
    pub link_by_email: bool,
}

impl Default for ProvisionPolicy {
    fn default() -> Self {
        Self {
            auto_provision: true,
            link_by_email: false,
        }
    }
}

pub trait AuthProvider {
    fn sign_in(
        &self,
        proof: SignInProof,
    ) -> impl Future<Output = Result<ResolvedIdentity, AuthError>> + Send;
}

impl<T: AuthProvider + Sync> AuthProvider for &T {
    fn sign_in(
        &self,
        proof: SignInProof,
    ) -> impl Future<Output = Result<ResolvedIdentity, AuthError>> + Send {
        (*self).sign_in(proof)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoProvider;

impl AuthProvider for NoProvider {
    async fn sign_in(&self, _proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        Err(AuthError::SourceDisabled)
    }
}

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

#[cfg(test)]
mod tests {
    use super::{AuthError, AuthProvider, AuthSources, NoProvider, ResolvedIdentity, SignInProof};
    use crate::credential::{Email, Issuer, Password, Subject};

    struct Fixed(Issuer);

    impl AuthProvider for Fixed {
        async fn sign_in(&self, _proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
            Ok(ResolvedIdentity {
                issuer: self.0.clone(),
                subject: Subject::try_from("subject").expect("a subject"),
                email: Email::try_from("person@example.com").expect("an address"),
                email_verified: false,
            })
        }
    }

    fn a_password_proof() -> SignInProof {
        SignInProof::Password {
            email: Email::try_from("person@example.com").expect("an address"),
            password: Password::new("correct horse battery"),
        }
    }

    fn a_federated_proof() -> SignInProof {
        SignInProof::Federated {
            code: "code".to_owned(),
            code_verifier: "verifier".to_owned(),
            nonce: "nonce".to_owned(),
            redirect_uri: "https://app.example.com/callback".to_owned(),
        }
    }

    fn local() -> Fixed {
        Fixed(Issuer::Local)
    }

    fn federated() -> Fixed {
        Fixed(Issuer::federated("https://id.example").expect("a url"))
    }

    #[tokio::test]
    async fn each_proof_goes_to_its_own_source() {
        let sources = AuthSources::new(Some(local()), Some(federated()));

        let by_password = sources
            .resolve(a_password_proof())
            .await
            .expect("the local source answers");
        let by_redirect = sources
            .resolve(a_federated_proof())
            .await
            .expect("the federated source answers");

        assert!(by_password.issuer.is_local());
        assert_eq!(by_redirect.issuer.as_str(), "https://id.example");
    }

    #[tokio::test]
    async fn a_proof_without_its_source_is_refused() {
        let only_federated: AuthSources<Fixed, Fixed> = AuthSources::new(None, Some(federated()));
        let only_local: AuthSources<Fixed, Fixed> = AuthSources::new(Some(local()), None);

        assert_eq!(
            only_federated.resolve(a_password_proof()).await,
            Err(AuthError::SourceDisabled)
        );
        assert_eq!(
            only_local.resolve(a_federated_proof()).await,
            Err(AuthError::SourceDisabled)
        );
        assert!(only_federated.resolve(a_federated_proof()).await.is_ok());
        assert!(only_local.resolve(a_password_proof()).await.is_ok());
    }

    #[tokio::test]
    async fn a_disabled_provider_refuses_everything() {
        assert_eq!(
            NoProvider.sign_in(a_password_proof()).await,
            Err(AuthError::SourceDisabled)
        );
    }

    #[test]
    fn a_proof_never_renders_its_credentials() {
        let rendered = format!("{:?}", a_password_proof());

        assert_eq!(rendered, "SignInProof::Password(redacted)");
        assert!(!rendered.contains("correct horse"));
        assert!(!rendered.contains("person@example.com"));
    }

    #[test]
    fn the_sources_report_only_which_are_enabled() {
        let sources: AuthSources<Fixed, Fixed> = AuthSources::new(Some(local()), None);

        assert_eq!(
            format!("{sources:?}"),
            "AuthSources { local: true, federated: false }"
        );
    }
}
