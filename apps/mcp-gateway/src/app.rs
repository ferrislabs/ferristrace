use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use bytes::Bytes;
use jiff::Timestamp;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::clock::{Clock, LocalTime, local_time};
use crate::config::Config;
use crate::credentials::ClientCredentials;
use crate::decision::{DecisionRecord, DecisionSink, KIND};
use crate::exchange::Exchanger;
use crate::pdp::{Pdp, PdpError, Verdict};
use crate::pip::{Facts, Needs, Pip};
use crate::reason::Reason;
use crate::rpc::{self, Kind, Message, ToolCall};
use crate::upstream::{HOP_BY_HOP, REQUEST_ID, Upstream, UpstreamError, natures_of};
use crate::verifier::{JwksVerifier, Principal, Refusal};
use crate::vocab::{self, ACTION, Nature};

const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_LIST_BYTES: usize = 1024 * 1024;
const METADATA_PREFIX: &str = "/.well-known/oauth-protected-resource";

#[derive(Debug, thiserror::Error)]
#[error("the gateway could not be built: {0}")]
pub struct BuildError(pub String);

#[derive(Debug)]
pub struct Gateway<C, S> {
    public_base: String,
    issuer: String,
    scopes: Vec<String>,
    verifier: JwksVerifier,
    pdp: Pdp,
    pip: Pip,
    exchanger: Exchanger,
    upstream: Upstream,
    clock: C,
    sink: S,
}

impl<C: Clock, S: DecisionSink> Gateway<C, S> {
    pub fn new(config: &Config, clock: C, sink: S) -> Result<Self, BuildError> {
        let failure = |what: &str| BuildError(what.to_owned());
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| failure("the HTTP client"))?;
        let upstream_http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(config.upstream_timeout())
            .build()
            .map_err(|_| failure("the upstream HTTP client"))?;
        let verifier = JwksVerifier::new(&config.issuer).map_err(|_| failure("issuer"))?;
        let api = reqwest::Url::parse(&config.api_url).map_err(|_| failure("api_url"))?;
        let base = reqwest::Url::parse(&config.upstream).map_err(|_| failure("upstream"))?;
        let pdp = Pdp::new(
            http.clone(),
            config.evaluation_url(),
            config.evaluations_url(),
            ClientCredentials::new(
                http.clone(),
                config.pdp_token_url(),
                config.pdp.client_id.clone(),
                config.pdp.client_secret.clone(),
                None,
                config.pdp_timeout(),
            ),
            config.pdp_timeout(),
        );
        let pip = Pip::new(
            http.clone(),
            api,
            ClientCredentials::new(
                http.clone(),
                config.exchange_token_url(),
                config.exchange.client_id.clone(),
                config.exchange.client_secret.clone(),
                Some(config.agent_audience.clone()),
                config.pdp_timeout(),
            ),
            config.pdp_timeout(),
        );
        let exchanger = Exchanger::new(
            http,
            config.exchange_token_url(),
            config.exchange.client_id.clone(),
            config.exchange.client_secret.clone(),
            config.agent_audience.clone(),
            config.exchange_timeout(),
        );
        Ok(Self {
            public_base: config.public_base().to_owned(),
            issuer: config.issuer.trim_end_matches('/').to_owned(),
            scopes: config.scopes_supported.clone(),
            verifier,
            pdp,
            pip,
            exchanger,
            upstream: Upstream::new(upstream_http, base),
            clock,
            sink,
        })
    }

    pub fn canonical(&self, org: Uuid) -> String {
        format!("{}/orgs/{org}/mcp", self.public_base)
    }

    pub fn metadata_url(&self, org: Uuid) -> String {
        format!("{}{METADATA_PREFIX}/orgs/{org}/mcp", self.public_base)
    }

    fn unauthorized(&self, org: Uuid, invalid_token: bool) -> Response {
        let mut parts = Vec::new();
        if invalid_token {
            parts.push("error=\"invalid_token\"".to_owned());
        }
        parts.push(format!("resource_metadata=\"{}\"", self.metadata_url(org)));
        if !self.scopes.is_empty() {
            parts.push(format!("scope=\"{}\"", self.scopes.join(" ")));
        }
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", parts.join(", "))) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }
        response
    }

    fn metadata(&self, org: Uuid) -> Value {
        json!({
            "resource": self.canonical(org),
            "authorization_servers": [self.issuer],
            "scopes_supported": self.scopes,
            "bearer_methods_supported": ["header"],
        })
    }

    fn now(&self) -> Timestamp {
        Timestamp::from_second(self.clock.now().as_second()).unwrap_or(Timestamp::UNIX_EPOCH)
    }

    fn record(&self, ctx: &Ctx<'_>, entry: &Entry<'_>) {
        let (subject_type, subject) = vocab::subject_kind(ctx.principal);
        self.sink.record(DecisionRecord {
            kind: KIND,
            time: self.now().to_string(),
            request_id: ctx.request_id.to_owned(),
            method: entry.method.to_owned(),
            org: ctx.org.to_string(),
            tool: entry.tool.map(str::to_owned),
            subject_type,
            subject: subject.to_owned(),
            groups: ctx.principal.groups.clone(),
            agent: ctx.principal.client_id.clone(),
            delegated: ctx.principal.delegated,
            act: ctx.principal.act.clone(),
            target: entry.target.clone(),
            arguments: entry.arguments.clone(),
            decision: if entry.reason.is_none() {
                "permit"
            } else {
                "deny"
            },
            policies: entry.policies.to_vec(),
            reason: entry.reason.map(Reason::code),
        });
    }

    fn refuse(
        &self,
        ctx: &Ctx<'_>,
        message: &Message,
        entry: Entry<'_>,
        status: StatusCode,
    ) -> Response {
        let reason = entry.reason.unwrap_or(Reason::NoPermittingPolicy);
        self.record(ctx, &entry);
        (status, Json(rpc::refusal(&message.id, reason))).into_response()
    }

    async fn exchange(&self, ctx: &Ctx<'_>) -> Result<String, UpstreamError> {
        self.exchanger
            .exchange(ctx.token, ctx.principal.expires_at)
            .await
            .map_err(|error| {
                tracing::warn!(%error, request_id = ctx.request_id, "the token exchange failed");
                UpstreamError
            })
    }

    async fn handle(&self, org: Uuid, request: Request) -> Response {
        let (parts, body) = request.into_parts();
        let request_id = request_id(&parts.headers);
        let Some(token) = bearer(&parts.headers) else {
            return self.unauthorized(org, false);
        };
        let principal = match self.verifier.verify(token, &self.canonical(org)).await {
            Ok(principal) => principal,
            Err(Refusal::Invalid) => return self.unauthorized(org, true),
            Err(Refusal::Unavailable) => return plain(StatusCode::SERVICE_UNAVAILABLE),
        };
        if parts.method != Method::POST {
            let mut response = StatusCode::METHOD_NOT_ALLOWED.into_response();
            response
                .headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("POST"));
            return response;
        }
        let Ok(body) = to_bytes(body, MAX_BODY_BYTES).await else {
            return plain(StatusCode::PAYLOAD_TOO_LARGE);
        };
        let ctx = Ctx {
            org,
            principal: &principal,
            token,
            request_id: &request_id,
        };
        let mut response = self.post(&ctx, &parts, body).await;
        if let Ok(value) = HeaderValue::from_str(&request_id) {
            response
                .headers_mut()
                .insert(HeaderName::from_static(REQUEST_ID), value);
        }
        response
    }

    async fn post(&self, ctx: &Ctx<'_>, parts: &Parts, body: Bytes) -> Response {
        let message = match rpc::parse(&body) {
            Ok(message) => message,
            Err(rejection) => {
                tracing::info!(
                    request_id = ctx.request_id,
                    code = rejection.code,
                    "request rejected"
                );
                return (StatusCode::BAD_REQUEST, Json(rejection.body())).into_response();
            }
        };
        let tool = message.call.as_ref().map(|call| call.name.as_str());
        if !rpc::headers_agree(&parts.headers, &message) {
            return self.refuse(
                ctx,
                &message,
                Entry::denied(&message.method, tool, Reason::HeaderMismatch),
                StatusCode::BAD_REQUEST,
            );
        }
        match message.kind {
            Kind::Pass => self.pass(ctx, parts, body).await,
            Kind::Other => self.refuse(
                ctx,
                &message,
                Entry::denied(&message.method, tool, Reason::MethodNotAllowed),
                StatusCode::OK,
            ),
            Kind::ListTools => self.list(ctx, parts, body, &message).await,
            Kind::CallTool => match &message.call {
                Some(call) => self.call(ctx, parts, body, &message, call).await,
                None => (
                    StatusCode::BAD_REQUEST,
                    Json(rpc::refusal(&message.id, Reason::InvalidTarget)),
                )
                    .into_response(),
            },
        }
    }

    async fn pass(&self, ctx: &Ctx<'_>, parts: &Parts, body: Bytes) -> Response {
        let Ok(exchanged) = self.exchange(ctx).await else {
            return upstream_unavailable();
        };
        self.send(ctx, parts, body, &exchanged).await
    }

    async fn send(&self, ctx: &Ctx<'_>, parts: &Parts, body: Bytes, exchanged: &str) -> Response {
        match self
            .upstream
            .forward(
                ctx.org,
                Method::POST,
                &parts.headers,
                body,
                exchanged,
                ctx.request_id,
            )
            .await
        {
            Ok(response) => stream_back(response),
            Err(_) => upstream_unavailable(),
        }
    }

    async fn facts(&self, ctx: &Ctx<'_>, needs: &Needs<'_>) -> Result<(Facts, LocalTime), Reason> {
        let facts = self
            .pip
            .gather(ctx.org, needs)
            .await
            .map_err(|_| Reason::PipUnavailable)?;
        let time = local_time(self.clock.now(), &facts.timezone).map_err(|error| {
            tracing::warn!(%error, request_id = ctx.request_id, "the organisation's time zone is unusable");
            Reason::PipUnavailable
        })?;
        Ok((facts, time))
    }

    async fn list(&self, ctx: &Ctx<'_>, parts: &Parts, body: Bytes, message: &Message) -> Response {
        let needs = Needs {
            oncall_email: oncall_email(ctx.principal),
            ..Needs::default()
        };
        let (facts, time) = match self.facts(ctx, &needs).await {
            Ok(found) => found,
            Err(reason) => {
                return self.refuse(
                    ctx,
                    message,
                    Entry::denied(&message.method, None, reason),
                    StatusCode::OK,
                );
            }
        };
        let Ok(exchanged) = self.exchange(ctx).await else {
            return upstream_unavailable();
        };
        let Ok(upstream) = self
            .upstream
            .forward(
                ctx.org,
                Method::POST,
                &parts.headers,
                body,
                &exchanged,
                ctx.request_id,
            )
            .await
        else {
            return upstream_unavailable();
        };
        let (status, mut headers, mut answer) = match listing(upstream).await {
            Listing::Relay(response) => return response,
            Listing::Json {
                status,
                headers,
                answer,
            } => (status, headers, answer),
        };
        match self.screen(ctx, message, &facts, &time, &mut answer).await {
            Ok(()) => rebuilt(status, &mut headers, &answer),
            Err(reason) => self.refuse(
                ctx,
                message,
                Entry::denied(&message.method, None, reason),
                StatusCode::OK,
            ),
        }
    }

    async fn screen(
        &self,
        ctx: &Ctx<'_>,
        message: &Message,
        facts: &Facts,
        time: &LocalTime,
        answer: &mut Value,
    ) -> Result<(), Reason> {
        let empty = Map::new();
        let Some(tools) = answer
            .get_mut("result")
            .and_then(|result| result.get_mut("tools"))
            .and_then(Value::as_array_mut)
        else {
            return Ok(());
        };
        tools.retain(|tool| tool.get("name").and_then(Value::as_str).is_some());
        let names: Vec<String> = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect();
        let resources: Vec<Value> = names
            .iter()
            .zip(tools.iter())
            .map(|(name, tool)| {
                let nature = Nature::from_annotations(tool.get("annotations"));
                vocab::resource(ctx.org, name, nature)
            })
            .collect();
        if let Some(found) = natures_of(answer) {
            self.upstream.remember(found);
        }
        let subject = vocab::subject(ctx.principal, facts.oncall.as_ref());
        let context = vocab::context(ctx.principal, &empty, &empty, time);
        let verdicts = self
            .pdp
            .evaluate_many(&subject, ACTION, &resources, &context)
            .await
            .map_err(PdpError::reason)?;
        for (name, verdict) in names.iter().zip(&verdicts) {
            self.record(
                ctx,
                &Entry::decided(&message.method, Some(name), &empty, &empty, verdict),
            );
        }
        let mut allowed = verdicts.iter().map(|verdict| verdict.allowed);
        if let Some(tools) = answer
            .get_mut("result")
            .and_then(|result| result.get_mut("tools"))
            .and_then(Value::as_array_mut)
        {
            tools.retain(|_| allowed.next().unwrap_or(false));
        }
        if let Some(result) = answer.get_mut("result").and_then(Value::as_object_mut) {
            result.insert("cacheScope".to_owned(), json!("private"));
        }
        Ok(())
    }

    async fn nature(&self, ctx: &Ctx<'_>, tool: &str) -> Result<Option<Nature>, UpstreamError> {
        if let Some(nature) = self.upstream.nature_of(tool) {
            return Ok(Some(nature));
        }
        if self.upstream.is_warm() {
            return Ok(None);
        }
        let exchanged = self.exchange(ctx).await?;
        let found = self
            .upstream
            .discover(ctx.org, &exchanged, ctx.request_id)
            .await?;
        let nature = found.get(tool).copied();
        self.upstream.remember(found);
        Ok(nature)
    }

    async fn call(
        &self,
        ctx: &Ctx<'_>,
        parts: &Parts,
        body: Bytes,
        message: &Message,
        call: &ToolCall,
    ) -> Response {
        let arguments = vocab::primitives(&call.arguments);
        let name = call.name.as_str();
        let method = message.method.as_str();
        let refuse = |reason: Reason, target: &Map<String, Value>| {
            self.refuse(
                ctx,
                message,
                Entry {
                    target,
                    arguments: &arguments,
                    ..Entry::denied(method, Some(name), reason)
                },
                StatusCode::OK,
            )
        };
        let plan = match vocab::plan(name, &call.arguments) {
            Ok(plan) => plan,
            Err(reason) => return refuse(reason, &Map::new()),
        };
        let nature = match self.nature(ctx, name).await {
            Ok(Some(nature)) => nature,
            Ok(None) => return refuse(Reason::UnknownTool, &plan.target),
            Err(_) => return upstream_unavailable(),
        };
        let needs = Needs {
            oncall_email: oncall_email(ctx.principal),
            incident: plan.incident,
            service: plan.service.as_deref(),
            fresh: !nature.read_only,
        };
        let (facts, time) = match self.facts(ctx, &needs).await {
            Ok(found) => found,
            Err(reason) => return refuse(reason, &plan.target),
        };
        let mut target = plan.target.clone();
        if let Some(incident) = &facts.incident {
            vocab::apply_incident(&mut target, incident);
        }
        if let Some(placement) = &facts.placement {
            vocab::apply_placement(&mut target, placement);
        }
        let subject = vocab::subject(ctx.principal, facts.oncall.as_ref());
        let resource = vocab::resource(ctx.org, name, nature);
        let context = vocab::context(ctx.principal, &arguments, &target, &time);
        let verdict = match self
            .pdp
            .evaluate(&subject, ACTION, &resource, &context)
            .await
        {
            Ok(verdict) => verdict,
            Err(error) => return refuse(error.reason(), &target),
        };
        if !verdict.allowed {
            return self.refuse(
                ctx,
                message,
                Entry::decided(method, Some(name), &target, &arguments, &verdict),
                StatusCode::OK,
            );
        }
        self.record(
            ctx,
            &Entry::decided(method, Some(name), &target, &arguments, &verdict),
        );
        let Ok(exchanged) = self.exchange(ctx).await else {
            return upstream_unavailable();
        };
        self.send(ctx, parts, body, &exchanged).await
    }
}

#[derive(Debug)]
struct Ctx<'a> {
    org: Uuid,
    principal: &'a Principal,
    token: &'a str,
    request_id: &'a str,
}

#[derive(Clone, Copy)]
struct Entry<'a> {
    method: &'a str,
    tool: Option<&'a str>,
    target: &'a Map<String, Value>,
    arguments: &'a Map<String, Value>,
    policies: &'a [String],
    reason: Option<Reason>,
}

static NOTHING: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);

impl<'a> Entry<'a> {
    fn denied(method: &'a str, tool: Option<&'a str>, reason: Reason) -> Self {
        Self {
            method,
            tool,
            target: &NOTHING,
            arguments: &NOTHING,
            policies: &[],
            reason: Some(reason),
        }
    }

    fn decided(
        method: &'a str,
        tool: Option<&'a str>,
        target: &'a Map<String, Value>,
        arguments: &'a Map<String, Value>,
        verdict: &'a Verdict,
    ) -> Self {
        Self {
            method,
            tool,
            target,
            arguments,
            policies: &verdict.policies,
            reason: (!verdict.allowed).then_some(Reason::NoPermittingPolicy),
        }
    }
}

fn oncall_email(principal: &Principal) -> Option<&str> {
    if principal.delegated {
        principal.email.as_deref()
    } else {
        None
    }
}

pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    let (scheme, token) = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer")
        && !token.is_empty()
        && !token.contains(char::is_whitespace))
    .then_some(token)
}

fn request_id(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 128 && value.is_ascii())
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_graphic()))
        .map_or_else(|| Uuid::new_v4().to_string(), str::to_owned)
}

fn plain(status: StatusCode) -> Response {
    status.into_response()
}

fn upstream_unavailable() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({ "error": "upstream unavailable" })),
    )
        .into_response()
}

enum Listing {
    Relay(Response),
    Json {
        status: StatusCode,
        headers: HeaderMap,
        answer: Value,
    },
}

async fn listing(upstream: reqwest::Response) -> Listing {
    if !upstream.status().is_success() {
        return Listing::Relay(stream_back(upstream));
    }
    let is_json = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"));
    if !is_json {
        return Listing::Relay(upstream_unavailable());
    }
    let status = upstream.status();
    let mut headers = upstream.headers().clone();
    let Some(bytes) = read_bounded(upstream).await else {
        return Listing::Relay(upstream_unavailable());
    };
    let Ok(answer) = serde_json::from_slice::<Value>(&bytes) else {
        return Listing::Relay(upstream_unavailable());
    };
    if answer
        .get("result")
        .and_then(|result| result.get("tools"))
        .is_some_and(Value::is_array)
    {
        return Listing::Json {
            status,
            headers,
            answer,
        };
    }
    if answer.get("result").is_none() && answer.get("error").is_some_and(Value::is_object) {
        return Listing::Relay(rebuilt(status, &mut headers, &answer));
    }
    Listing::Relay(upstream_unavailable())
}

async fn read_bounded(mut response: reqwest::Response) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > MAX_LIST_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

fn no_store(headers: &mut HeaderMap) {
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::VARY, HeaderValue::from_static("authorization"));
}

fn rebuilt(status: StatusCode, upstream: &mut HeaderMap, answer: &Value) -> Response {
    let mut response = Response::builder().status(status);
    for (name, value) in upstream.iter() {
        let name_str = name.as_str();
        if !HOP_BY_HOP.contains(&name_str)
            && name_str != "content-length"
            && name_str != "content-encoding"
        {
            response = response.header(name, value);
        }
    }
    let Ok(mut response) = response.body(Body::from(answer.to_string())) else {
        return upstream_unavailable();
    };
    no_store(response.headers_mut());
    response
}

fn stream_back(upstream: reqwest::Response) -> Response {
    let mut response = Response::builder().status(upstream.status());
    for (name, value) in upstream.headers() {
        if !HOP_BY_HOP.contains(&name.as_str()) {
            response = response.header(name, value);
        }
    }
    response
        .body(Body::from_stream(upstream.bytes_stream()))
        .unwrap_or_else(|_| upstream_unavailable())
}

pub fn router<C: Clock, S: DecisionSink>(gateway: Arc<Gateway<C, S>>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route(
            "/.well-known/oauth-protected-resource/orgs/{org}/mcp",
            get(metadata::<C, S>),
        )
        .route("/orgs/{org}/mcp", any(mcp::<C, S>))
        .with_state(gateway)
}

fn organisation(raw: &str) -> Option<Uuid> {
    (raw.len() == 36)
        .then(|| Uuid::parse_str(raw).ok())
        .flatten()
}

async fn metadata<C: Clock, S: DecisionSink>(
    State(gateway): State<Arc<Gateway<C, S>>>,
    Path(org): Path<String>,
) -> Response {
    match organisation(&org) {
        Some(org) => Json(gateway.metadata(org)).into_response(),
        None => plain(StatusCode::NOT_FOUND),
    }
}

async fn mcp<C: Clock, S: DecisionSink>(
    State(gateway): State<Arc<Gateway<C, S>>>,
    Path(org): Path<String>,
    request: Request,
) -> Response {
    match organisation(&org) {
        Some(org) => gateway.handle(org, request).await,
        None => plain(StatusCode::NOT_FOUND),
    }
}
