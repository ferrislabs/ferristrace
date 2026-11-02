use super::auth_sources::AuthSources;
use super::errors::AuthError;
use super::ports::{AuthProvider, NoProvider};
use super::value_objects::{Email, Issuer, Password, ResolvedIdentity, SignInProof, Subject};

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
