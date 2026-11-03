#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{Form, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ferristrace_mcp_gateway::clock::Clock;
use ferristrace_mcp_gateway::config::{Config, ExchangeConfig, PdpConfig};
use ferristrace_mcp_gateway::decision::{DecisionRecord, DecisionSink};
use ferristrace_mcp_gateway::{Gateway, Secret, router};
use jiff::Timestamp;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::EncodePrivateKey;
use serde_json::{Value, json};

pub const ORG: &str = "11111111-1111-4111-8111-111111111111";
pub const OTHER_ORG: &str = "99999999-9999-4999-8999-999999999999";
pub const INCIDENT: &str = "22222222-2222-4222-8222-222222222222";
pub const PUBLIC: &str = "https://mcp.example.com";
pub const AUDIENCE: &str = "ferristrace";
pub const KID: &str = "test-key";

const KEY_BYTES: [u8; 32] = [7; 32];
const OTHER_KEY_BYTES: [u8; 32] = [9; 32];

pub fn canonical(org: &str) -> String {
    format!("{PUBLIC}/orgs/{org}/mcp")
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HashMap<String, String>,
    pub raw: String,
    pub body: Value,
    pub form: HashMap<String, String>,
}

impl Seen {
    pub fn header(&self, name: &str) -> &str {
        self.headers.get(name).map_or("", String::as_str)
    }

    pub fn rpc_method(&self) -> &str {
        self.body["method"].as_str().unwrap_or("")
    }
}

#[derive(Clone, Default)]
pub struct Log(Arc<Mutex<Vec<Seen>>>);

impl Log {
    pub fn push(&self, seen: Seen) {
        if seen.path == "/" {
            return;
        }
        self.0.lock().unwrap().push(seen);
    }

    pub fn all(&self) -> Vec<Seen> {
        self.0.lock().unwrap().clone()
    }

    pub fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    pub fn to_path(&self, suffix: &str) -> Vec<Seen> {
        self.all()
            .into_iter()
            .filter(|seen| seen.path.ends_with(suffix))
            .collect()
    }

    pub fn rpc(&self, method: &str) -> Vec<Seen> {
        self.all()
            .into_iter()
            .filter(|seen| seen.rpc_method() == method)
            .collect()
    }

    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

async fn capture(request: Request) -> (Seen, Request<axum::body::Body>) {
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 1 << 22).await.unwrap();
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let seen = Seen {
        method: parts.method.to_string(),
        path: parts.uri.path().to_owned(),
        query: parts.uri.query().unwrap_or("").to_owned(),
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        form: serde_urlencoded_pairs(&raw),
        raw,
    };
    (
        seen,
        Request::from_parts(parts, axum::body::Body::from(bytes)),
    )
}

fn serde_urlencoded_pairs(raw: &str) -> HashMap<String, String> {
    raw.split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (decode(key), decode(value)))
        .collect()
}

fn decode(raw: &str) -> String {
    let bytes = raw.replace('+', " ");
    let bytes = bytes.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

pub fn closed_port_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{address}")
}

#[derive(Debug, Clone)]
pub struct Verdict {
    pub allow: bool,
    pub policies: Vec<String>,
}

impl Verdict {
    pub fn permit(policy: &str) -> Self {
        Self {
            allow: true,
            policies: vec![policy.to_owned()],
        }
    }

    pub fn deny(policy: &str) -> Self {
        Self {
            allow: false,
            policies: vec![policy.to_owned()],
        }
    }
}

#[derive(Clone)]
pub enum Pdp {
    Rule(fn(&Value) -> Verdict),
    Status(u16),
    Slow(u64, fn(&Value) -> Verdict),
    Body(String),
    ShortBatch,
}

#[derive(Clone, Default)]
pub struct TokenBehaviour {
    pub failing_clients: Vec<String>,
    pub empty_clients: Vec<String>,
}

pub struct Realm {
    pub url: String,
    pub issuer: String,
    pub log: Log,
    pub pdp: Mutex<Pdp>,
    pub token: Mutex<TokenBehaviour>,
    pub jwks_failing: Mutex<bool>,
    pub jwks_fetches: AtomicUsize,
    exchanges: AtomicUsize,
}

fn secret_key(bytes: [u8; 32]) -> SecretKey {
    SecretKey::from_slice(&bytes).unwrap()
}

pub fn jwk() -> Value {
    let point = secret_key(KEY_BYTES).public_key().to_encoded_point(false);
    json!({
        "kty": "EC", "crv": "P-256", "use": "sig", "alg": "ES256", "kid": KID,
        "x": URL_SAFE_NO_PAD.encode(point.x().unwrap()),
        "y": URL_SAFE_NO_PAD.encode(point.y().unwrap()),
    })
}

fn signer(bytes: [u8; 32]) -> EncodingKey {
    let der = secret_key(bytes).to_pkcs8_der().unwrap();
    EncodingKey::from_ec_der(der.as_bytes())
}

pub fn sign(claims: &Value) -> String {
    sign_with(Algorithm::ES256, Some(KID), claims, &signer(KEY_BYTES))
}

pub fn sign_rogue(claims: &Value) -> String {
    sign_with(
        Algorithm::ES256,
        Some(KID),
        claims,
        &signer(OTHER_KEY_BYTES),
    )
}

pub fn sign_with_kid(kid: Option<&str>, claims: &Value) -> String {
    sign_with(Algorithm::ES256, kid, claims, &signer(KEY_BYTES))
}

pub fn sign_hs256(claims: &Value) -> String {
    sign_with(
        Algorithm::HS256,
        Some(KID),
        claims,
        &EncodingKey::from_secret(b"shared-secret"),
    )
}

fn sign_with(algorithm: Algorithm, kid: Option<&str>, claims: &Value, key: &EncodingKey) -> String {
    let mut header = Header::new(algorithm);
    header.kid = kid.map(str::to_owned);
    jsonwebtoken::encode(&header, claims, key).unwrap()
}

type Shared = Arc<Realm>;

async fn discovery(State(realm): State<Shared>) -> Response {
    axum::Json(json!({"issuer": realm.issuer, "jwks_uri": format!("{}/certs", realm.issuer)}))
        .into_response()
}

async fn certs(State(realm): State<Shared>) -> Response {
    realm.jwks_fetches.fetch_add(1, Ordering::SeqCst);
    if *realm.jwks_failing.lock().unwrap() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    axum::Json(json!({"keys": [jwk()]})).into_response()
}

async fn token(State(realm): State<Shared>, Form(form): Form<HashMap<String, String>>) -> Response {
    let client = form.get("client_id").cloned().unwrap_or_default();
    let expected = match client.as_str() {
        "pep" => "pep-secret",
        "gateway" => "gateway-secret",
        _ => "",
    };
    let seen = Seen {
        method: "POST".to_owned(),
        path: "/token".to_owned(),
        query: String::new(),
        headers: HashMap::new(),
        raw: String::new(),
        body: Value::Null,
        form: form.clone(),
    };
    realm.log.push(seen);
    let behaviour = realm.token.lock().unwrap().clone();
    if form.get("client_secret").map(String::as_str) != Some(expected)
        || behaviour.failing_clients.contains(&client)
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if behaviour.empty_clients.contains(&client) {
        return axum::Json(json!({"access_token": "", "expires_in": 60})).into_response();
    }
    let grant = form.get("grant_type").map_or("", String::as_str);
    let access = if grant == "client_credentials" {
        match form.get("audience") {
            Some(audience) => format!("cc-{client}-{audience}"),
            None => format!("cc-{client}"),
        }
    } else {
        let number = realm.exchanges.fetch_add(1, Ordering::SeqCst) + 1;
        format!("exchanged-{number}")
    };
    axum::Json(json!({"access_token": access, "expires_in": 300, "token_type": "Bearer"}))
        .into_response()
}

fn full_request(defaults: &Value, item: Option<&Value>) -> Value {
    let mut full = json!({
        "subject": defaults["subject"],
        "action": defaults["action"],
        "resource": defaults["resource"],
        "context": defaults["context"],
    });
    if let Some(item) = item {
        for key in ["subject", "action", "resource", "context"] {
            if item.get(key).is_some() {
                full[key] = item[key].clone();
            }
        }
    }
    full
}

fn decision_json(verdict: &Verdict) -> Value {
    json!({"decision": verdict.allow, "context": {"reason_admin": {"policies": verdict.policies}}})
}

async fn evaluate(State(realm): State<Shared>, request: Request) -> Response {
    answer(realm, request, false).await
}

async fn evaluations(State(realm): State<Shared>, request: Request) -> Response {
    answer(realm, request, true).await
}

async fn answer(realm: Shared, request: Request, batch: bool) -> Response {
    let (seen, _) = capture(request).await;
    let authorization = seen.header("authorization").to_owned();
    let body = seen.body.clone();
    realm.log.push(seen);
    if authorization != "Bearer cc-pep" {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let pdp = realm.pdp.lock().unwrap().clone();
    let (rule, delay) = match pdp {
        Pdp::Status(status) => return StatusCode::from_u16(status).unwrap().into_response(),
        Pdp::Body(text) => {
            return ([(header::CONTENT_TYPE, "application/json")], text).into_response();
        }
        Pdp::ShortBatch => return axum::Json(json!({"evaluations": []})).into_response(),
        Pdp::Slow(millis, rule) => (rule, Some(millis)),
        Pdp::Rule(rule) => (rule, None),
    };
    if let Some(millis) = delay {
        tokio::time::sleep(Duration::from_millis(millis)).await;
    }
    if batch {
        let items = body["evaluations"].as_array().cloned().unwrap_or_default();
        let decisions: Vec<Value> = items
            .iter()
            .map(|item| decision_json(&rule(&full_request(&body, Some(item)))))
            .collect();
        axum::Json(json!({"evaluations": decisions})).into_response()
    } else {
        axum::Json(decision_json(&rule(&full_request(&body, None)))).into_response()
    }
}

pub async fn start_realm() -> Arc<Realm> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let url = format!("http://{address}");
    let realm = Arc::new(Realm {
        issuer: format!("{url}/realms/ferris"),
        url,
        log: Log::default(),
        pdp: Mutex::new(Pdp::Rule(|_| Verdict::deny("default"))),
        token: Mutex::new(TokenBehaviour::default()),
        jwks_failing: Mutex::new(false),
        jwks_fetches: AtomicUsize::new(0),
        exchanges: AtomicUsize::new(0),
    });
    let app = Router::new()
        .route(
            "/realms/ferris/.well-known/openid-configuration",
            get(discovery),
        )
        .route("/realms/ferris/certs", get(certs))
        .route("/realms/ferris/protocol/openid-connect/token", post(token))
        .route(
            "/realms/ferris/authzen/access/v1/evaluation",
            post(evaluate),
        )
        .route(
            "/realms/ferris/authzen/access/v1/evaluations",
            post(evaluations),
        )
        .with_state(Arc::clone(&realm));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    realm
}

impl Realm {
    pub fn set_pdp(&self, pdp: Pdp) {
        *self.pdp.lock().unwrap() = pdp;
    }

    pub fn evaluations(&self) -> Vec<Seen> {
        self.log.to_path("/authzen/access/v1/evaluations")
    }

    pub fn evaluation(&self) -> Vec<Seen> {
        self.log.to_path("/authzen/access/v1/evaluation")
    }

    pub fn pdp_calls(&self) -> usize {
        self.evaluations().len() + self.evaluation().len()
    }

    pub fn exchanges(&self) -> Vec<Seen> {
        self.log
            .all()
            .into_iter()
            .filter(|seen| {
                seen.form
                    .get("grant_type")
                    .is_some_and(|grant| grant.contains("token-exchange"))
            })
            .collect()
    }

    pub fn client_credentials(&self) -> Vec<Seen> {
        self.log
            .all()
            .into_iter()
            .filter(|seen| {
                seen.form
                    .get("grant_type")
                    .is_some_and(|grant| grant == "client_credentials")
            })
            .collect()
    }

    pub fn token_calls(&self) -> usize {
        self.log
            .all()
            .iter()
            .filter(|seen| seen.path == "/token")
            .count()
    }
}

type Reply = (u16, String, u64);

#[derive(Clone, Default)]
pub struct Fake {
    pub log: Log,
    replies: Arc<Mutex<HashMap<String, Reply>>>,
    mode: Arc<Mutex<UpstreamMode>>,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub enum UpstreamMode {
    #[default]
    Normal,
    ListAsEventStream,
    ListGarbage,
    Down,
    ListError,
}

impl Fake {
    pub fn reply(&self, method: &str, path: &str, status: u16, body: &Value) {
        self.reply_after(method, path, status, &body.to_string(), 0);
    }

    pub fn reply_text(&self, method: &str, path: &str, status: u16, body: &str) {
        self.reply_after(method, path, status, body, 0);
    }

    pub fn reply_after(&self, method: &str, path: &str, status: u16, body: &str, delay_ms: u64) {
        self.replies.lock().unwrap().insert(
            format!("{method} {path}"),
            (status, body.to_owned(), delay_ms),
        );
    }

    pub fn set_mode(&self, mode: UpstreamMode) {
        *self.mode.lock().unwrap() = mode;
    }
}

async fn recorded(State(fake): State<Fake>, request: Request) -> Response {
    let (seen, _) = capture(request).await;
    let key = format!("{} {}", seen.method, seen.path);
    fake.log.push(seen);
    let reply = fake.replies.lock().unwrap().get(&key).cloned();
    match reply {
        Some((status, body, delay)) => {
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            (
                StatusCode::from_u16(status).unwrap(),
                [(header::CONTENT_TYPE, "application/json")],
                body,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "no such route").into_response(),
    }
}

pub async fn start_api() -> (Fake, String) {
    let fake = Fake::default();
    let url = serve(Router::new().fallback(recorded).with_state(fake.clone())).await;
    (fake, url)
}

pub fn tools() -> Value {
    json!([
        {"name": "search_logs", "annotations": {"readOnlyHint": true, "openWorldHint": false}},
        {"name": "list_services", "annotations": {"readOnlyHint": true}},
        {"name": "get_incident", "annotations": {"readOnlyHint": true}},
        {"name": "comment_incident", "annotations": {"readOnlyHint": false, "destructiveHint": false}},
        {"name": "resolve_incident", "annotations": {"readOnlyHint": false, "destructiveHint": true}},
        {"name": "who_is_on_call", "annotations": {"readOnlyHint": true}},
    ])
}

async fn upstream_mcp(State(fake): State<Fake>, request: Request) -> Response {
    let (seen, _) = capture(request).await;
    let body = seen.body.clone();
    fake.log.push(seen);
    let mode = fake.mode.lock().unwrap().clone();
    if mode == UpstreamMode::Down {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let id = body["id"].clone();
    let method = body["method"].as_str().unwrap_or("");
    let json_reply = |value: Value| {
        (
            [(header::CONTENT_TYPE, "application/json")],
            value.to_string(),
        )
            .into_response()
    };
    match method {
        "initialize" => json_reply(json!({"jsonrpc": "2.0", "id": id, "result": {
            "protocolVersion": "2025-11-25", "capabilities": {"tools": {}},
            "serverInfo": {"name": "fake", "version": "0"}}})),
        "ping" => json_reply(json!({"jsonrpc": "2.0", "id": id, "result": {}})),
        "tools/list" => match mode {
            UpstreamMode::ListAsEventStream => (
                [(header::CONTENT_TYPE, "text/event-stream")],
                format!(
                    "data: {}\n\n",
                    json!({"jsonrpc": "2.0", "id": id, "result": {"tools": tools()}})
                ),
            )
                .into_response(),
            UpstreamMode::ListGarbage => json_reply(json!("not an object")),
            UpstreamMode::ListError => json_reply(
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": "boom"}}),
            ),
            _ => json_reply(json!({"jsonrpc": "2.0", "id": id, "result": {"tools": tools()}})),
        },
        "tools/call" => json_reply(json!({"jsonrpc": "2.0", "id": id, "result": {
            "content": [{"type": "text", "text": format!("ran {}", body["params"]["name"].as_str().unwrap_or(""))}],
            "isError": false}})),
        _ if method.starts_with("notifications/") => StatusCode::ACCEPTED.into_response(),
        _ => json_reply(
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "no such method"}}),
        ),
    }
}

pub async fn start_upstream() -> (Fake, String) {
    let fake = Fake::default();
    let url = serve(
        Router::new()
            .fallback(upstream_mcp)
            .with_state(fake.clone()),
    )
    .await;
    (fake, url)
}

#[derive(Clone, Default)]
pub struct Capture(pub Arc<Mutex<Vec<DecisionRecord>>>);

impl Capture {
    pub fn all(&self) -> Vec<DecisionRecord> {
        self.0.lock().unwrap().clone()
    }
}

impl DecisionSink for Capture {
    fn record(&self, record: DecisionRecord) {
        self.0.lock().unwrap().push(record);
    }
}

pub struct World {
    pub realm: Arc<Realm>,
    pub upstream: Fake,
    pub api: Fake,
    pub decisions: Capture,
    pub url: String,
    pub http: reqwest::Client,
    pub config: Config,
}

pub fn base_config(realm: &Realm, upstream: &str, api: &str) -> Config {
    let defaults = Config::default();
    Config {
        public_url: PUBLIC.to_owned(),
        upstream: upstream.to_owned(),
        issuer: realm.issuer.clone(),
        api_url: api.to_owned(),
        agent_audience: AUDIENCE.to_owned(),
        pdp: PdpConfig {
            url: realm.url.clone(),
            realm: "ferris".to_owned(),
            client_id: "pep".to_owned(),
            client_secret: Secret::new("pep-secret"),
            timeout_ms: 400,
            ..defaults.pdp.clone()
        },
        exchange: ExchangeConfig {
            client_id: "gateway".to_owned(),
            client_secret: Secret::new("gateway-secret"),
            ..defaults.exchange.clone()
        },
        ..defaults
    }
}

pub fn clock_at(text: &str) -> FixedAt {
    FixedAt(text.parse().unwrap())
}

#[derive(Debug, Clone, Copy)]
pub struct FixedAt(pub Timestamp);

impl Clock for FixedAt {
    fn now(&self) -> Timestamp {
        self.0
    }
}

impl World {
    pub async fn start() -> Self {
        Self::start_at(clock_at("2026-11-03T09:05:00Z"), |_| {}).await
    }

    pub async fn start_at(clock: FixedAt, tweak: fn(&mut Config)) -> Self {
        let realm = start_realm().await;
        let (upstream, upstream_url) = start_upstream().await;
        let (api, api_url) = start_api().await;
        let mut config = base_config(&realm, &upstream_url, &api_url);
        tweak(&mut config);
        Self::assemble(realm, upstream, api, config, clock).await
    }

    pub async fn assemble(
        realm: Arc<Realm>,
        upstream: Fake,
        api: Fake,
        config: Config,
        clock: FixedAt,
    ) -> Self {
        api.reply(
            "GET",
            &format!("/api/orgs/{ORG}/clock"),
            200,
            &json!({"timezone": "Europe/Paris"}),
        );
        api.reply(
            "GET",
            &format!("/api/orgs/{ORG}/oncall/now"),
            200,
            &json!({"on_call": false, "schedules": []}),
        );
        let decisions = Capture::default();
        let gateway = Gateway::new(&config, clock, decisions.clone()).unwrap();
        let url = serve(router(Arc::new(gateway))).await;
        Self {
            realm,
            upstream,
            api,
            decisions,
            url,
            http: reqwest::Client::new(),
            config,
        }
    }

    pub fn claims(&self) -> Value {
        json!({
            "iss": self.realm.issuer,
            "sub": "alice-sub",
            "aud": canonical(ORG),
            "azp": "claude-code",
            "sid": "session-1",
            "email": "alice@example.com",
            "groups": ["Dev"],
            "roles": ["member"],
            "typ": "Bearer",
            "iat": now(),
            "exp": now() + 300,
        })
    }

    pub fn user_token(&self) -> String {
        sign(&self.claims())
    }

    pub fn client_token(&self) -> String {
        let mut claims = self.claims();
        let map = claims.as_object_mut().unwrap();
        map.remove("sid");
        map.remove("email");
        map.remove("groups");
        map.insert("sub".to_owned(), json!("service-account-claude-code"));
        sign(&claims)
    }

    pub fn endpoint(&self, org: &str) -> String {
        format!("{}/orgs/{org}/mcp", self.url)
    }

    pub async fn post(&self, token: Option<&str>, body: &Value) -> (StatusCode, HeaderMap, Value) {
        self.post_raw(token, body.to_string(), &[]).await
    }

    pub async fn post_raw(
        &self,
        token: Option<&str>,
        body: String,
        extra: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Value) {
        let mut request = self
            .http
            .post(self.endpoint(ORG))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        for (name, value) in extra {
            request = request.header(*name, *value);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await.unwrap();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (
            StatusCode::from_u16(status.as_u16()).unwrap(),
            headers,
            body,
        )
    }

    pub async fn rpc(&self, token: &str, method: &str, params: Value) -> Value {
        let (_, _, body) = self
            .post(
                Some(token),
                &json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}),
            )
            .await;
        body
    }

    pub async fn call(&self, token: &str, tool: &str, arguments: &Value) -> Value {
        self.rpc(
            token,
            "tools/call",
            json!({"name": tool, "arguments": arguments}),
        )
        .await
    }

    pub fn reply_api(&self, tail: &str, status: u16, body: &Value) {
        self.api
            .reply("GET", &format!("/api/orgs/{ORG}/{tail}"), status, body);
    }
}

pub fn read_only_tools(request: &Value) -> Verdict {
    if request["resource"]["properties"]["access"] == "read" {
        Verdict::permit("members-read")
    } else {
        Verdict::deny("members-read-only")
    }
}

pub fn permit_all(_: &Value) -> Verdict {
    Verdict::permit("permit-all")
}

pub fn deny_all(_: &Value) -> Verdict {
    Verdict::deny("deny-all")
}
