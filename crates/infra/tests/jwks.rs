use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ferristrace_domain::{AgentToken, AgentTokens, TokenRefusal};
use ferristrace_infra::{JwksSettings, JwksVerifier};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::EncodePrivateKey;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const FIRST: &str = "first-rsa";
const SECOND: &str = "second-rsa";
const ELLIPTIC: &str = "the-ec-key";
const CLIENT: &str = "triage-bot";
const AUDIENCE: &str = "ferristrace";

static FIRST_KEY: LazyLock<RsaPrivateKey> =
    LazyLock::new(|| RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).expect("a key"));
static SECOND_KEY: LazyLock<RsaPrivateKey> =
    LazyLock::new(|| RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).expect("a key"));
static EC_KEY: LazyLock<SecretKey> =
    LazyLock::new(|| SecretKey::random(&mut rsa::rand_core::OsRng));

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs()
}

fn rsa_jwk(kid: &str, key: &RsaPrivateKey) -> Value {
    let public = RsaPublicKey::from(key);
    json!({
        "kty": "RSA",
        "use": "sig",
        "alg": "RS256",
        "kid": kid,
        "n": URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
        "e": URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())
    })
}

fn ec_jwk(kid: &str) -> Value {
    let point = EC_KEY.public_key().to_encoded_point(false);
    json!({
        "kty": "EC",
        "crv": "P-256",
        "use": "sig",
        "alg": "ES256",
        "kid": kid,
        "x": URL_SAFE_NO_PAD.encode(point.x().expect("an x")),
        "y": URL_SAFE_NO_PAD.encode(point.y().expect("a y"))
    })
}

fn rsa_signer(key: &RsaPrivateKey) -> EncodingKey {
    let der = key.to_pkcs1_der().expect("the key encodes");
    EncodingKey::from_rsa_der(der.as_bytes())
}

fn ec_signer() -> EncodingKey {
    let der = EC_KEY.to_pkcs8_der().expect("the key encodes");
    EncodingKey::from_ec_der(der.as_bytes())
}

fn default_claims(issuer: &str) -> Value {
    json!({
        "iss": issuer,
        "sub": "7f0a0c1e-0000-4000-8000-000000000001",
        "aud": [AUDIENCE],
        "azp": CLIENT,
        "scope": "telemetry:read incidents:comment",
        "jti": "6b1d4a52-0000-4000-8000-000000000002",
        "typ": "Bearer",
        "iat": now(),
        "exp": now() + 300
    })
}

fn sign(alg: Algorithm, kid: Option<&str>, claims: &Value, key: &EncodingKey) -> AgentToken {
    let mut header = Header::new(alg);
    header.kid = kid.map(str::to_owned);
    let token = jsonwebtoken::encode(&header, claims, key).expect("the token signs");
    AgentToken::from_plaintext(&token)
}

#[derive(Clone)]
struct Server {
    issuer: Arc<Mutex<String>>,
    advertised: Arc<Mutex<Option<String>>>,
    keys: Arc<Mutex<Vec<Value>>>,
    key_fetches: Arc<AtomicUsize>,
    discoveries: Arc<AtomicUsize>,
    failing: Arc<AtomicBool>,
    oversize: Arc<AtomicBool>,
    delay_ms: Arc<AtomicU64>,
    keys_host: Arc<Mutex<Option<String>>>,
}

impl Server {
    fn publish(&self, keys: Vec<Value>) {
        *self.keys.lock().expect("the lock") = keys;
    }

    fn fetched(&self) -> usize {
        self.key_fetches.load(Ordering::SeqCst)
    }

    fn issuer(&self) -> String {
        self.issuer.lock().expect("the lock").clone()
    }
}

async fn discovery(State(server): State<Server>) -> Json<Value> {
    server.discoveries.fetch_add(1, Ordering::SeqCst);
    let issuer = server.issuer();
    let advertised = server
        .advertised
        .lock()
        .expect("the lock")
        .clone()
        .unwrap_or_else(|| issuer.clone());
    let uri = match server.keys_host.lock().expect("the lock").clone() {
        Some(host) => format!("http://{host}/realms/ferris/certs"),
        None => format!("{issuer}/certs"),
    };
    Json(json!({ "issuer": advertised, "jwks_uri": uri }))
}

async fn certs(State(server): State<Server>) -> axum::response::Response {
    use axum::response::IntoResponse;
    server.key_fetches.fetch_add(1, Ordering::SeqCst);
    let delay = server.delay_ms.load(Ordering::SeqCst);
    if delay > 0 {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    if server.failing.load(Ordering::SeqCst) {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if server.oversize.load(Ordering::SeqCst) {
        return Json(json!({ "keys": [], "padding": "x".repeat(2 << 20) })).into_response();
    }
    let keys = server.keys.lock().expect("the lock").clone();
    Json(json!({ "keys": keys })).into_response()
}

async fn serve(keys: Vec<Value>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let issuer = format!(
        "http://{}/realms/ferris",
        listener.local_addr().expect("an address")
    );
    let server = Server {
        issuer: Arc::new(Mutex::new(issuer)),
        advertised: Arc::new(Mutex::new(None)),
        keys: Arc::new(Mutex::new(keys)),
        key_fetches: Arc::new(AtomicUsize::new(0)),
        discoveries: Arc::new(AtomicUsize::new(0)),
        failing: Arc::new(AtomicBool::new(false)),
        oversize: Arc::new(AtomicBool::new(false)),
        delay_ms: Arc::new(AtomicU64::new(0)),
        keys_host: Arc::new(Mutex::new(None)),
    };
    let router = Router::new()
        .route(
            "/realms/ferris/.well-known/openid-configuration",
            get(discovery),
        )
        .route("/realms/ferris/certs", get(certs))
        .with_state(server.clone());
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("the issuer runs");
    });
    server
}

async fn issuer_with_keys() -> Server {
    serve(vec![rsa_jwk(FIRST, &FIRST_KEY), ec_jwk(ELLIPTIC)]).await
}

fn verifier(server: &Server, audience: Option<&str>) -> JwksVerifier {
    JwksVerifier::new(JwksSettings {
        issuer: server.issuer(),
        audience: audience.map(str::to_owned),
    })
    .expect("a usable issuer")
}

#[tokio::test]
async fn an_rs256_token_from_the_issuer_verifies_and_yields_its_claims() {
    let server = issuer_with_keys().await;
    let claims = default_claims(&server.issuer());
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );

    let verified = verifier(&server, Some(AUDIENCE))
        .verify(&token)
        .await
        .expect("a valid token");

    assert_eq!(&*verified.client_id, CLIENT);
    assert_eq!(
        verified.scopes,
        vec![
            Box::<str>::from("telemetry:read"),
            Box::<str>::from("incidents:comment")
        ]
    );
    assert_eq!(&*verified.jti, "6b1d4a52-0000-4000-8000-000000000002");
    assert_eq!(
        verified.expires_at_unix_secs,
        claims["exp"].as_u64().unwrap()
    );
}

#[tokio::test]
async fn an_es256_token_from_the_issuer_verifies() {
    let server = issuer_with_keys().await;
    let token = sign(
        Algorithm::ES256,
        Some(ELLIPTIC),
        &default_claims(&server.issuer()),
        &ec_signer(),
    );

    let verified = verifier(&server, None).verify(&token).await;

    assert_eq!(verified.map(|token| token.client_id), Ok(CLIENT.into()));
}

#[tokio::test]
async fn the_client_id_claim_stands_in_when_there_is_no_azp() {
    let server = issuer_with_keys().await;
    let mut claims = default_claims(&server.issuer());
    claims.as_object_mut().expect("an object").remove("azp");
    claims["client_id"] = json!("from-client-id");
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );

    let verified = verifier(&server, None).verify(&token).await.expect("valid");

    assert_eq!(&*verified.client_id, "from-client-id");
}

#[tokio::test]
async fn a_token_naming_no_client_is_refused() {
    let server = issuer_with_keys().await;
    let mut claims = default_claims(&server.issuer());
    claims.as_object_mut().expect("an object").remove("azp");
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );

    assert_eq!(
        verifier(&server, None).verify(&token).await,
        Err(TokenRefusal::Invalid)
    );
}

#[tokio::test]
async fn a_token_without_scope_verifies_with_no_scopes() {
    let server = issuer_with_keys().await;
    let mut claims = default_claims(&server.issuer());
    claims.as_object_mut().expect("an object").remove("scope");
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );

    let verified = verifier(&server, None).verify(&token).await.expect("valid");

    assert!(verified.scopes.is_empty());
}

#[tokio::test]
async fn a_scope_claim_listed_as_an_array_is_read_too() {
    let server = issuer_with_keys().await;
    let mut claims = default_claims(&server.issuer());
    claims["scope"] = json!(["telemetry:read"]);
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );

    let verified = verifier(&server, None).verify(&token).await.expect("valid");

    assert_eq!(verified.scopes, vec![Box::<str>::from("telemetry:read")]);
}

#[tokio::test]
async fn a_token_signed_by_another_key_is_refused() {
    let server = issuer_with_keys().await;
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &default_claims(&server.issuer()),
        &rsa_signer(&SECOND_KEY),
    );

    assert_eq!(
        verifier(&server, None).verify(&token).await,
        Err(TokenRefusal::Invalid)
    );
}

#[tokio::test]
async fn a_token_with_a_tampered_payload_is_refused() {
    let server = issuer_with_keys().await;
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &default_claims(&server.issuer()),
        &rsa_signer(&FIRST_KEY),
    );
    let mut parts: Vec<&str> = token.exposed().split('.').collect();
    let forged = URL_SAFE_NO_PAD.encode(
        json!({
            "iss": server.issuer(), "azp": "admin-bot", "jti": "x",
            "scope": "telemetry:read", "exp": now() + 300
        })
        .to_string(),
    );
    parts[1] = &forged;
    let tampered = AgentToken::from_plaintext(&parts.join("."));

    assert_eq!(
        verifier(&server, None).verify(&tampered).await,
        Err(TokenRefusal::Invalid)
    );
}

#[tokio::test]
async fn an_expired_token_is_refused_beyond_the_leeway() {
    let server = issuer_with_keys().await;
    let mut claims = default_claims(&server.issuer());
    claims["exp"] = json!(now() - 3600);
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );

    assert_eq!(
        verifier(&server, None).verify(&token).await,
        Err(TokenRefusal::Invalid)
    );
}

#[tokio::test]
async fn a_token_without_an_expiry_is_refused() {
    let server = issuer_with_keys().await;
    let mut claims = default_claims(&server.issuer());
    claims.as_object_mut().expect("an object").remove("exp");
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );

    assert_eq!(
        verifier(&server, None).verify(&token).await,
        Err(TokenRefusal::Invalid)
    );
}

#[tokio::test]
async fn a_token_from_another_issuer_is_refused() {
    let server = issuer_with_keys().await;
    for issuer in [
        "http://elsewhere.example.com/realms/ferris".to_owned(),
        format!("{}/", server.issuer()),
        String::new(),
    ] {
        let mut claims = default_claims(&server.issuer());
        claims["iss"] = json!(issuer);
        let token = sign(
            Algorithm::RS256,
            Some(FIRST),
            &claims,
            &rsa_signer(&FIRST_KEY),
        );

        assert_eq!(
            verifier(&server, None).verify(&token).await,
            Err(TokenRefusal::Invalid),
            "{issuer}"
        );
    }
}

#[tokio::test]
async fn a_token_without_an_issuer_is_refused() {
    let server = issuer_with_keys().await;
    let mut claims = default_claims(&server.issuer());
    claims.as_object_mut().expect("an object").remove("iss");
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );

    assert_eq!(
        verifier(&server, None).verify(&token).await,
        Err(TokenRefusal::Invalid)
    );
}

#[tokio::test]
async fn the_audience_is_enforced_only_when_configured() {
    let server = issuer_with_keys().await;
    let mut other = default_claims(&server.issuer());
    other["aud"] = json!(["somebody-else"]);
    let mut none = default_claims(&server.issuer());
    none.as_object_mut().expect("an object").remove("aud");
    let mut bare = default_claims(&server.issuer());
    bare["aud"] = json!(AUDIENCE);
    let mut several = default_claims(&server.issuer());
    several["aud"] = json!(["somebody-else", AUDIENCE]);
    let signer = rsa_signer(&FIRST_KEY);
    let wrong = sign(Algorithm::RS256, Some(FIRST), &other, &signer);
    let missing = sign(Algorithm::RS256, Some(FIRST), &none, &signer);
    let single = sign(Algorithm::RS256, Some(FIRST), &bare, &signer);
    let among = sign(Algorithm::RS256, Some(FIRST), &several, &signer);

    let strict = verifier(&server, Some(AUDIENCE));
    let lax = verifier(&server, None);

    assert_eq!(strict.verify(&wrong).await, Err(TokenRefusal::Invalid));
    assert_eq!(strict.verify(&missing).await, Err(TokenRefusal::Invalid));
    assert!(strict.verify(&single).await.is_ok());
    assert!(strict.verify(&among).await.is_ok());
    assert!(lax.verify(&wrong).await.is_ok());
    assert!(lax.verify(&missing).await.is_ok());
}

#[tokio::test]
async fn a_token_declaring_no_algorithm_is_refused() {
    let server = issuer_with_keys().await;
    let header = URL_SAFE_NO_PAD.encode(json!({"alg": "none", "kid": FIRST}).to_string());
    let payload = URL_SAFE_NO_PAD.encode(default_claims(&server.issuer()).to_string());
    let verifier = verifier(&server, None);

    for spelled in [
        format!("{header}.{payload}."),
        format!("{header}.{payload}.c2ln"),
    ] {
        assert_eq!(
            verifier.verify(&AgentToken::from_plaintext(&spelled)).await,
            Err(TokenRefusal::Invalid),
            "{spelled}"
        );
    }
}

#[tokio::test]
async fn a_symmetric_token_is_refused_even_signed_with_the_public_key() {
    let server = issuer_with_keys().await;
    let claims = default_claims(&server.issuer());
    let public = RsaPublicKey::from(&*FIRST_KEY);
    let published = [
        public.n().to_bytes_be(),
        b"secret".to_vec(),
        FIRST_KEY
            .to_pkcs1_der()
            .expect("encodes")
            .as_bytes()
            .to_vec(),
    ];
    let verifier = verifier(&server, None);

    for algorithm in [Algorithm::HS256, Algorithm::HS384, Algorithm::HS512] {
        for secret in &published {
            let token = sign(
                algorithm,
                Some(FIRST),
                &claims,
                &EncodingKey::from_secret(secret),
            );

            assert_eq!(
                verifier.verify(&token).await,
                Err(TokenRefusal::Invalid),
                "{algorithm:?}"
            );
        }
    }
}

#[tokio::test]
async fn other_asymmetric_algorithms_are_refused() {
    let server = issuer_with_keys().await;
    let claims = default_claims(&server.issuer());
    let signer = rsa_signer(&FIRST_KEY);
    let verifier = verifier(&server, None);

    for algorithm in [Algorithm::RS384, Algorithm::RS512, Algorithm::PS256] {
        let token = sign(algorithm, Some(FIRST), &claims, &signer);

        assert_eq!(
            verifier.verify(&token).await,
            Err(TokenRefusal::Invalid),
            "{algorithm:?}"
        );
    }
}

#[tokio::test]
async fn a_key_is_only_used_with_its_own_algorithm() {
    let server = issuer_with_keys().await;
    let claims = default_claims(&server.issuer());
    let elliptic_for_rsa = sign(Algorithm::ES256, Some(FIRST), &claims, &ec_signer());
    let rsa_for_elliptic = sign(
        Algorithm::RS256,
        Some(ELLIPTIC),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );
    let verifier = verifier(&server, None);

    assert_eq!(
        verifier.verify(&elliptic_for_rsa).await,
        Err(TokenRefusal::Invalid)
    );
    assert_eq!(
        verifier.verify(&rsa_for_elliptic).await,
        Err(TokenRefusal::Invalid)
    );
}

#[tokio::test]
async fn a_token_without_a_key_id_a_token_id_or_a_bearer_type_is_refused() {
    let server = issuer_with_keys().await;
    let signer = rsa_signer(&FIRST_KEY);
    let verifier = verifier(&server, None);
    let no_kid = sign(
        Algorithm::RS256,
        None,
        &default_claims(&server.issuer()),
        &signer,
    );
    let mut without_jti = default_claims(&server.issuer());
    without_jti
        .as_object_mut()
        .expect("an object")
        .remove("jti");
    let no_jti = sign(Algorithm::RS256, Some(FIRST), &without_jti, &signer);
    let mut blank_jti = default_claims(&server.issuer());
    blank_jti["jti"] = json!("");
    let empty_jti = sign(Algorithm::RS256, Some(FIRST), &blank_jti, &signer);
    let mut refresh = default_claims(&server.issuer());
    refresh["typ"] = json!("Refresh");
    let refresh = sign(Algorithm::RS256, Some(FIRST), &refresh, &signer);
    let mut identity = default_claims(&server.issuer());
    identity["typ"] = json!("ID");
    let identity = sign(Algorithm::RS256, Some(FIRST), &identity, &signer);

    for token in [no_kid, no_jti, empty_jti, refresh, identity] {
        assert_eq!(verifier.verify(&token).await, Err(TokenRefusal::Invalid));
    }
}

#[tokio::test]
async fn garbage_is_refused_without_reaching_the_issuer() {
    let server = issuer_with_keys().await;
    let verifier = verifier(&server, None);

    for garbage in ["", "a.b.c", "....", "not a token", "eyJ.eyJ.sig"] {
        assert_eq!(
            verifier.verify(&AgentToken::from_plaintext(garbage)).await,
            Err(TokenRefusal::Invalid),
            "{garbage}"
        );
    }
    assert_eq!(server.fetched(), 0);
}

#[tokio::test]
async fn keys_are_cached_across_verifications() {
    let server = issuer_with_keys().await;
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &default_claims(&server.issuer()),
        &rsa_signer(&FIRST_KEY),
    );
    let verifier = verifier(&server, None);

    for _ in 0..5 {
        verifier.verify(&token).await.expect("valid");
    }

    assert_eq!(server.fetched(), 1);
    assert_eq!(server.discoveries.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_unknown_key_id_refetches_once_and_then_at_most_once_per_gap() {
    let server = issuer_with_keys().await;
    let rotated_keys = vec![rsa_jwk(FIRST, &FIRST_KEY), rsa_jwk(SECOND, &SECOND_KEY)];
    let verifier = verifier(&server, None)
        .with_cache_timing(Duration::from_secs(600), Duration::from_millis(1500));
    let claims = default_claims(&server.issuer());
    let known = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );
    let rotated = sign(
        Algorithm::RS256,
        Some(SECOND),
        &claims,
        &rsa_signer(&SECOND_KEY),
    );
    let unknown = sign(
        Algorithm::RS256,
        Some("nobody"),
        &claims,
        &rsa_signer(&SECOND_KEY),
    );
    verifier.verify(&known).await.expect("valid");
    assert_eq!(server.fetched(), 1);

    server.publish(rotated_keys);
    let too_soon = verifier.verify(&rotated).await;
    assert_eq!(too_soon, Err(TokenRefusal::Invalid));
    assert_eq!(server.fetched(), 1);

    tokio::time::sleep(Duration::from_millis(1600)).await;
    let after_the_gap = verifier.verify(&rotated).await;
    assert!(after_the_gap.is_ok(), "{after_the_gap:?}");
    assert_eq!(server.fetched(), 2);

    for _ in 0..5 {
        assert_eq!(verifier.verify(&unknown).await, Err(TokenRefusal::Invalid));
    }
    assert_eq!(server.fetched(), 2);

    tokio::time::sleep(Duration::from_millis(1600)).await;
    assert_eq!(verifier.verify(&unknown).await, Err(TokenRefusal::Invalid));
    assert_eq!(server.fetched(), 3);
    for _ in 0..5 {
        assert_eq!(verifier.verify(&unknown).await, Err(TokenRefusal::Invalid));
    }
    assert_eq!(server.fetched(), 3);
}

#[tokio::test]
async fn a_key_the_issuer_stopped_publishing_is_refused_after_the_cache_lapses() {
    let server = issuer_with_keys().await;
    let rotated = rsa_jwk(SECOND, &SECOND_KEY);
    let verifier = verifier(&server, None)
        .with_cache_timing(Duration::from_millis(1500), Duration::from_millis(100));
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &default_claims(&server.issuer()),
        &rsa_signer(&FIRST_KEY),
    );
    verifier.verify(&token).await.expect("valid");

    server.publish(vec![rotated]);
    verifier.verify(&token).await.expect("still cached");
    tokio::time::sleep(Duration::from_millis(1600)).await;

    assert_eq!(verifier.verify(&token).await, Err(TokenRefusal::Invalid));
    assert_eq!(server.fetched(), 2);
}

#[tokio::test]
async fn an_unreachable_issuer_makes_the_verifier_unavailable_not_permissive() {
    let server = issuer_with_keys().await;
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &default_claims(&server.issuer()),
        &rsa_signer(&FIRST_KEY),
    );
    let dead = JwksVerifier::new(JwksSettings {
        issuer: "http://127.0.0.1:1/realms/ferris".to_owned(),
        audience: None,
    })
    .expect("a usable url");

    assert_eq!(dead.verify(&token).await, Err(TokenRefusal::Unavailable));
}

#[tokio::test]
async fn a_discovery_document_naming_another_issuer_is_not_trusted() {
    let server = issuer_with_keys().await;
    *server.advertised.lock().expect("the lock") =
        Some("http://elsewhere.example.com/realms/ferris".to_owned());
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &default_claims(&server.issuer()),
        &rsa_signer(&FIRST_KEY),
    );

    assert_eq!(
        verifier(&server, None).verify(&token).await,
        Err(TokenRefusal::Unavailable)
    );
    assert_eq!(server.fetched(), 0);
}

#[tokio::test]
async fn encryption_keys_and_unsupported_keys_are_never_used_to_verify() {
    let mut encryption = rsa_jwk(FIRST, &FIRST_KEY);
    encryption["use"] = json!("enc");
    let server = serve(vec![
        json!({"kty": "oct", "kid": "shared", "k": "c2VjcmV0"}),
        json!({"kty": "OKP", "crv": "Ed25519", "kid": "edwards", "x": "AAAA"}),
        json!({"kty": "nonsense"}),
        encryption,
        rsa_jwk(SECOND, &SECOND_KEY),
    ])
    .await;
    let claims = default_claims(&server.issuer());
    let verifier = verifier(&server, None);

    let encrypted = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );
    let shared = sign(
        Algorithm::HS256,
        Some("shared"),
        &claims,
        &EncodingKey::from_secret(b"secret"),
    );
    let good = sign(
        Algorithm::RS256,
        Some(SECOND),
        &claims,
        &rsa_signer(&SECOND_KEY),
    );

    assert_eq!(
        verifier.verify(&encrypted).await,
        Err(TokenRefusal::Invalid)
    );
    assert_eq!(verifier.verify(&shared).await, Err(TokenRefusal::Invalid));
    assert!(verifier.verify(&good).await.is_ok());
}

#[tokio::test]
async fn an_issuer_with_a_trailing_slash_is_the_same_issuer() {
    let server = issuer_with_keys().await;
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &default_claims(&server.issuer()),
        &rsa_signer(&FIRST_KEY),
    );
    let verifier = JwksVerifier::new(JwksSettings {
        issuer: format!("{}/", server.issuer()),
        audience: None,
    })
    .expect("a usable url");

    assert!(verifier.verify(&token).await.is_ok());
}

#[test]
fn an_issuer_that_is_not_a_url_is_refused_at_start() {
    for issuer in [
        "",
        "ferriskey",
        "ftp://issuer.example.com",
        "/realms/ferris",
    ] {
        let built = JwksVerifier::new(JwksSettings {
            issuer: issuer.to_owned(),
            audience: None,
        });

        assert!(built.is_err(), "{issuer}");
    }
}

#[tokio::test]
async fn a_failed_refetch_keeps_the_cached_keys_verifiable() {
    let server = issuer_with_keys().await;
    let verifier = verifier(&server, None)
        .with_cache_timing(Duration::from_millis(1500), Duration::from_millis(100));
    let claims = default_claims(&server.issuer());
    let known = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );
    let unknown = sign(
        Algorithm::RS256,
        Some("nobody"),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );
    verifier.verify(&known).await.expect("valid");

    server.failing.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let refused = verifier.verify(&unknown).await;
    tokio::time::sleep(Duration::from_millis(1600)).await;
    let after_the_ttl = verifier.verify(&known).await;

    assert_eq!(refused, Err(TokenRefusal::Unavailable));
    assert!(after_the_ttl.is_ok(), "{after_the_ttl:?}");
    assert!(server.fetched() >= 2);
}

#[tokio::test]
async fn others_keep_verifying_with_cached_keys_while_a_refetch_is_in_flight() {
    let server = issuer_with_keys().await;
    let verifier = Arc::new(
        verifier(&server, None)
            .with_cache_timing(Duration::from_secs(600), Duration::from_millis(100)),
    );
    let claims = default_claims(&server.issuer());
    let known = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );
    let unknown = sign(
        Algorithm::RS256,
        Some("nobody"),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );
    verifier.verify(&known).await.expect("valid");
    server.delay_ms.store(1500, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let slow = {
        let verifier = Arc::clone(&verifier);
        tokio::spawn(async move { verifier.verify(&unknown).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;

    let started = std::time::Instant::now();
    let meanwhile = verifier.verify(&known).await;

    assert!(meanwhile.is_ok());
    assert!(started.elapsed() < Duration::from_millis(1000));
    assert_eq!(slow.await.expect("joined"), Err(TokenRefusal::Invalid));
}

#[tokio::test]
async fn a_token_not_yet_valid_is_refused() {
    let server = issuer_with_keys().await;
    let mut claims = default_claims(&server.issuer());
    claims["nbf"] = json!(now() + 3600);
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &claims,
        &rsa_signer(&FIRST_KEY),
    );

    assert_eq!(
        verifier(&server, None).verify(&token).await,
        Err(TokenRefusal::Invalid)
    );
}

#[tokio::test]
async fn an_oversized_key_document_is_not_read() {
    let server = issuer_with_keys().await;
    server.oversize.store(true, Ordering::SeqCst);
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &default_claims(&server.issuer()),
        &rsa_signer(&FIRST_KEY),
    );

    assert_eq!(
        verifier(&server, None).verify(&token).await,
        Err(TokenRefusal::Unavailable)
    );
}

#[tokio::test]
async fn keys_served_from_another_host_than_the_issuer_are_not_trusted() {
    let server = issuer_with_keys().await;
    let port = server
        .issuer()
        .split(':')
        .nth(2)
        .expect("a port")
        .split('/')
        .next()
        .expect("a port")
        .to_owned();
    *server.keys_host.lock().expect("the lock") = Some(format!("localhost:{port}"));
    let token = sign(
        Algorithm::RS256,
        Some(FIRST),
        &default_claims(&server.issuer()),
        &rsa_signer(&FIRST_KEY),
    );

    assert_eq!(
        verifier(&server, None).verify(&token).await,
        Err(TokenRefusal::Unavailable)
    );
    assert_eq!(server.fetched(), 0);
}
