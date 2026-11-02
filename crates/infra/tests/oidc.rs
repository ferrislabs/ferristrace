use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::{get, post};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ferristrace_domain::{AuthError, AuthProvider, SignInProof};
use ferristrace_infra::{OidcProvider, OidcSettings};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const KEY_ID: &str = "the-test-key";
const CLIENT_ID: &str = "ferristrace";
const SUBJECT: &str = "subject-123";
const ADDRESS: &str = "person@example.com";
const CODE: &str = "the-authorization-code";

#[derive(Clone)]
struct Provider {
    issuer: String,
    key: Arc<RsaPrivateKey>,
    nonce: Arc<Mutex<String>>,
}

async fn metadata(State(provider): State<Provider>) -> Json<Value> {
    let issuer = provider.issuer;
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "scopes_supported": ["openid", "email"],
        "claims_supported": ["sub", "email", "email_verified"]
    }))
}

async fn jwks(State(provider): State<Provider>) -> Json<Value> {
    let public = RsaPublicKey::from(provider.key.as_ref());
    Json(json!({
        "keys": [{
            "kty": "RSA",
            "use": "sig",
            "alg": "RS256",
            "kid": KEY_ID,
            "n": URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
            "e": URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())
        }]
    }))
}

async fn token(State(provider): State<Provider>) -> Json<Value> {
    let nonce = provider.nonce.lock().expect("the nonce lock").clone();
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_secs(),
    )
    .expect("a representable instant");
    let claims = json!({
        "iss": provider.issuer,
        "aud": CLIENT_ID,
        "sub": SUBJECT,
        "exp": now + 3600,
        "iat": now,
        "nonce": nonce,
        "email": ADDRESS,
        "email_verified": true
    });

    let der = provider
        .key
        .to_pkcs1_der()
        .expect("the key encodes")
        .as_bytes()
        .to_vec();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KEY_ID.to_owned());
    let id_token = jsonwebtoken::encode(&header, &claims, &EncodingKey::from_rsa_der(&der))
        .expect("the token signs");

    Json(json!({
        "access_token": "the-access-token",
        "token_type": "Bearer",
        "expires_in": 3600,
        "id_token": id_token
    }))
}

async fn a_provider() -> (OidcProvider, Arc<Mutex<String>>) {
    let key = Arc::new(RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).expect("a key"));
    let nonce = Arc::new(Mutex::new(String::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let issuer = format!("http://{}", listener.local_addr().expect("an address"));

    let state = Provider {
        issuer: issuer.clone(),
        key,
        nonce: Arc::clone(&nonce),
    };
    let router = Router::new()
        .route("/.well-known/openid-configuration", get(metadata))
        .route("/jwks", get(jwks))
        .route("/token", post(token))
        .with_state(state);
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("the provider runs");
    });

    let provider = OidcProvider::discover(OidcSettings {
        issuer,
        client_id: CLIENT_ID.to_owned(),
        client_secret: "the-secret".to_owned(),
        redirect_uri: "http://app.example.com/callback".to_owned(),
    })
    .await
    .expect("discovery answers");

    (provider, nonce)
}

#[tokio::test]
async fn an_authorization_url_carries_pkce_and_the_scopes() {
    let (provider, _nonce) = a_provider().await;

    let (url, flow) = provider.begin().expect("an authorization url");

    assert!(url.contains("response_type=code"), "{url}");
    assert!(url.contains("code_challenge_method=S256"), "{url}");
    assert!(url.contains("code_challenge="), "{url}");
    assert!(url.contains("scope=openid+email"), "{url}");
    assert!(url.contains(&format!("state={}", flow.state)), "{url}");
    assert!(url.contains(&format!("nonce={}", flow.nonce)), "{url}");
    assert!(
        !url.contains(flow.verifier.exposed()),
        "the verifier must stay home"
    );
}

#[tokio::test]
async fn a_callback_resolves_the_federated_identity() {
    let (provider, nonce) = a_provider().await;
    let (_url, flow) = provider.begin().expect("an authorization url");
    *nonce.lock().expect("the nonce lock") = flow.nonce.clone();

    let identity = provider
        .sign_in(SignInProof {
            code: CODE.to_owned(),
            code_verifier: flow.verifier.exposed().to_owned(),
            nonce: flow.nonce.clone(),
            redirect_uri: flow.redirect_uri.clone(),
        })
        .await
        .expect("the provider answers");

    assert!(identity.issuer.as_str().starts_with("http://127.0.0.1:"));
    assert_eq!(identity.subject.as_str(), SUBJECT);
    assert_eq!(identity.email.as_str(), ADDRESS);
    assert!(identity.email_verified);
}

#[tokio::test]
async fn a_callback_carrying_the_wrong_nonce_is_refused() {
    let (provider, nonce) = a_provider().await;
    let (_url, flow) = provider.begin().expect("an authorization url");
    *nonce.lock().expect("the nonce lock") = "a-nonce-nobody-asked-for".to_owned();

    let refused = provider
        .sign_in(SignInProof {
            code: CODE.to_owned(),
            code_verifier: flow.verifier.exposed().to_owned(),
            nonce: flow.nonce.clone(),
            redirect_uri: flow.redirect_uri.clone(),
        })
        .await;

    assert_eq!(refused, Err(AuthError::Rejected));
}
