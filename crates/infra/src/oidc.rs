use ferristrace_domain::{
    AuthError, AuthProvider, Authorizer, CodeVerifier, Email, Issuer, PendingFlow,
    ResolvedIdentity, SignInProof, Subject,
};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::reqwest;
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};

const EMAIL: &str = "email";
const PROFILE: &str = "profile";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcSettings {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    #[error("the issuer, the client or the redirect address is not a usable url")]
    Settings,
    #[error("the provider did not answer discovery")]
    Discovery,
}

#[derive(Debug, Clone)]
pub struct OidcProvider {
    metadata: CoreProviderMetadata,
    settings: OidcSettings,
    http: reqwest::Client,
    issuer: Issuer,
}

impl OidcProvider {
    pub async fn discover(settings: OidcSettings) -> Result<Self, OidcError> {
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| OidcError::Settings)?;
        let url = IssuerUrl::new(settings.issuer.clone()).map_err(|_| OidcError::Settings)?;
        let metadata = CoreProviderMetadata::discover_async(url, &http)
            .await
            .map_err(|_| OidcError::Discovery)?;
        let issuer = Issuer::federated(&settings.issuer).map_err(|_| OidcError::Settings)?;

        Ok(Self {
            metadata,
            settings,
            http,
            issuer,
        })
    }

    pub fn begin(&self) -> Result<(String, PendingFlow), OidcError> {
        let redirect = RedirectUrl::new(self.settings.redirect_uri.clone())
            .map_err(|_| OidcError::Settings)?;
        let client = CoreClient::from_provider_metadata(
            self.metadata.clone(),
            ClientId::new(self.settings.client_id.clone()),
            Some(ClientSecret::new(self.settings.client_secret.clone())),
        )
        .set_redirect_uri(redirect);

        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new(EMAIL.to_owned()))
            .add_scope(Scope::new(PROFILE.to_owned()))
            .set_pkce_challenge(challenge)
            .url();

        Ok((
            url.to_string(),
            PendingFlow {
                state: state.secret().clone(),
                nonce: nonce.secret().clone(),
                verifier: CodeVerifier::from_plaintext(verifier.secret()),
                redirect_uri: self.settings.redirect_uri.clone(),
            },
        ))
    }
}

impl AuthProvider for OidcProvider {
    async fn sign_in(&self, proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        let SignInProof::Federated {
            code,
            code_verifier,
            nonce,
            redirect_uri,
        } = proof
        else {
            return Err(AuthError::SourceDisabled);
        };

        let redirect = RedirectUrl::new(redirect_uri).map_err(|_| AuthError::Rejected)?;
        let client = CoreClient::from_provider_metadata(
            self.metadata.clone(),
            ClientId::new(self.settings.client_id.clone()),
            Some(ClientSecret::new(self.settings.client_secret.clone())),
        )
        .set_redirect_uri(redirect);

        let tokens = client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|_| AuthError::ProviderUnreachable)?
            .set_pkce_verifier(PkceCodeVerifier::new(code_verifier))
            .request_async(&self.http)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "the provider refused the code exchange");
                AuthError::Rejected
            })?;

        let token = tokens.id_token().ok_or_else(|| {
            tracing::warn!("the provider returned no id token");
            AuthError::Rejected
        })?;
        let verifier = client.id_token_verifier();
        let claims = token
            .claims(&verifier, &Nonce::new(nonce))
            .map_err(|error| {
                tracing::warn!(%error, "the id token did not validate");
                AuthError::Rejected
            })?;

        if let Some(expected) = claims.access_token_hash() {
            let algorithm = token.signing_alg().map_err(|_| AuthError::Rejected)?;
            let key = token
                .signing_key(&verifier)
                .map_err(|_| AuthError::Rejected)?;
            let actual = AccessTokenHash::from_token(tokens.access_token(), algorithm, key)
                .map_err(|_| AuthError::Rejected)?;
            if actual != *expected {
                return Err(AuthError::Rejected);
            }
        }

        let address = claims.email().ok_or(AuthError::Rejected)?;
        Ok(ResolvedIdentity {
            issuer: self.issuer.clone(),
            subject: Subject::try_from(claims.subject().as_str())
                .map_err(|_| AuthError::Rejected)?,
            email: Email::try_from(address.as_str()).map_err(|_| AuthError::Rejected)?,
            email_verified: claims.email_verified().unwrap_or(false),
        })
    }
}

impl Authorizer for OidcProvider {
    fn begin(&self) -> Result<(String, PendingFlow), AuthError> {
        OidcProvider::begin(self).map_err(|error| {
            tracing::error!(%error, "an authorization url could not be built");
            AuthError::ProviderUnreachable
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidconnect::core::{
        CoreJwsSigningAlgorithm, CoreResponseType, CoreSubjectIdentifierType,
    };
    use openidconnect::{
        AuthUrl, EmptyAdditionalProviderMetadata, JsonWebKeySetUrl, ResponseTypes,
    };

    fn provider() -> OidcProvider {
        let metadata = CoreProviderMetadata::new(
            IssuerUrl::new("https://ferriskey.test".to_owned()).unwrap(),
            AuthUrl::new("https://ferriskey.test/authorize".to_owned()).unwrap(),
            JsonWebKeySetUrl::new("https://ferriskey.test/jwks".to_owned()).unwrap(),
            vec![ResponseTypes::new(vec![CoreResponseType::Code])],
            vec![CoreSubjectIdentifierType::Public],
            vec![CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256],
            EmptyAdditionalProviderMetadata {},
        );
        let settings = OidcSettings {
            issuer: "https://ferriskey.test".to_owned(),
            client_id: "ferristrace".to_owned(),
            client_secret: "secret".to_owned(),
            redirect_uri: "https://ferristrace.test/callback".to_owned(),
        };
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let issuer = Issuer::federated(&settings.issuer).unwrap();

        OidcProvider {
            metadata,
            settings,
            http,
            issuer,
        }
    }

    #[test]
    fn begin_requests_the_email_and_profile_scopes_beside_openid() {
        let (url, _) = provider().begin().unwrap();
        let parsed = openidconnect::url::Url::parse(&url).unwrap();
        let scope = parsed
            .query_pairs()
            .find(|(key, _)| key == "scope")
            .map(|(_, value)| value.into_owned())
            .unwrap();
        let requested: std::collections::HashSet<&str> = scope.split_whitespace().collect();

        assert!(requested.contains("openid"));
        assert!(requested.contains("email"));
        assert!(requested.contains("profile"));
    }
}
