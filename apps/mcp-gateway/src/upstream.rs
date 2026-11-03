use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, header};
use bytes::Bytes;
use reqwest::Url;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::rpc;
use crate::vocab::Nature;

pub const REQUEST_ID: &str = "x-request-id";
const NATURES_LIVE_FOR: Duration = Duration::from_secs(600);
const MAX_LIST_BYTES: usize = 1 << 20;

const FORWARDED: [&str; 3] = ["content-type", "accept", "last-event-id"];

pub const HOP_BY_HOP: [&str; 9] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the MCP server did not answer usably")]
pub struct UpstreamError;

pub type Natures = HashMap<String, Nature>;

#[derive(Debug)]
pub struct Upstream {
    http: reqwest::Client,
    base: Url,
    natures: Mutex<Option<(Instant, Arc<Natures>)>>,
}

impl Upstream {
    pub fn new(http: reqwest::Client, base: Url) -> Self {
        Self {
            http,
            base,
            natures: Mutex::new(None),
        }
    }

    fn endpoint(&self, org: Uuid) -> Url {
        let mut url = self.base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty()
                .extend(["orgs", &org.to_string(), "mcp"]);
        }
        url
    }

    pub fn nature_of(&self, tool: &str) -> Option<Nature> {
        let held = self
            .natures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (at, natures) = held.as_ref()?;
        (at.elapsed() < NATURES_LIVE_FOR)
            .then(|| natures.get(tool).copied())
            .flatten()
    }

    pub fn remember(&self, natures: Natures) {
        *self
            .natures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((Instant::now(), Arc::new(natures)));
    }

    pub fn is_warm(&self) -> bool {
        self.natures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < NATURES_LIVE_FOR)
    }

    pub async fn forward(
        &self,
        org: Uuid,
        method: Method,
        inbound: &HeaderMap,
        body: Bytes,
        token: &str,
        request_id: &str,
    ) -> Result<reqwest::Response, UpstreamError> {
        let headers = outgoing(inbound, token, request_id)?;
        self.http
            .request(method, self.endpoint(org))
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(%error, "the MCP server call failed");
                UpstreamError
            })
    }

    pub async fn discover(
        &self,
        org: Uuid,
        token: &str,
        request_id: &str,
    ) -> Result<Natures, UpstreamError> {
        let request = json!({"jsonrpc": "2.0", "id": "ferristrace-gateway-discovery", "method": "tools/list"});
        let body = rpc::with_meta(request.to_string().as_bytes()).ok_or(UpstreamError)?;
        let mut inbound = HeaderMap::new();
        inbound.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        inbound.insert(
            header::ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        inbound.insert(
            HeaderName::from_static("mcp-protocol-version"),
            HeaderValue::from_static(rpc::PROTOCOL_2026),
        );
        inbound.insert(
            HeaderName::from_static(rpc::MCP_METHOD),
            HeaderValue::from_static("tools/list"),
        );
        let mut response = self
            .forward(
                org,
                Method::POST,
                &inbound,
                Bytes::from(body),
                token,
                request_id,
            )
            .await?;
        if !response.status().is_success() {
            return Err(UpstreamError);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| UpstreamError)? {
            if bytes.len() + chunk.len() > MAX_LIST_BYTES {
                return Err(UpstreamError);
            }
            bytes.extend_from_slice(&chunk);
        }
        let message: Value = serde_json::from_slice(&bytes).map_err(|_| UpstreamError)?;
        natures_of(&message).ok_or(UpstreamError)
    }
}

pub fn natures_of(message: &Value) -> Option<Natures> {
    let tools = message.get("result")?.get("tools")?.as_array()?;
    Some(
        tools
            .iter()
            .filter_map(|tool| {
                let name = tool.get("name")?.as_str()?;
                Some((
                    name.to_owned(),
                    Nature::from_annotations(tool.get("annotations")),
                ))
            })
            .collect(),
    )
}

pub fn outgoing(
    inbound: &HeaderMap,
    token: &str,
    request_id: &str,
) -> Result<HeaderMap, UpstreamError> {
    let mut outgoing = HeaderMap::new();
    for (name, value) in inbound {
        let name_str = name.as_str();
        if FORWARDED.contains(&name_str) || name_str.starts_with("mcp-") {
            outgoing.append(name.clone(), value.clone());
        }
    }
    let mut bearer =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| UpstreamError)?;
    bearer.set_sensitive(true);
    outgoing.insert(header::AUTHORIZATION, bearer);
    outgoing.insert(
        HeaderName::from_static(REQUEST_ID),
        HeaderValue::from_str(request_id).map_err(|_| UpstreamError)?,
    );
    Ok(outgoing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caller_credentials_and_hop_by_hop_headers_never_travel() {
        let mut inbound = HeaderMap::new();
        inbound.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer caller-token"),
        );
        inbound.insert(header::COOKIE, HeaderValue::from_static("session=1"));
        inbound.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
        inbound.insert(header::HOST, HeaderValue::from_static("mcp.example.com"));
        inbound.insert(
            header::TRANSFER_ENCODING,
            HeaderValue::from_static("chunked"),
        );
        inbound.insert(
            header::PROXY_AUTHORIZATION,
            HeaderValue::from_static("Basic x"),
        );
        inbound.insert("x-forwarded-for", HeaderValue::from_static("1.2.3.4"));
        inbound.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        inbound.insert("mcp-method", HeaderValue::from_static("tools/call"));
        inbound.insert(
            "mcp-protocol-version",
            HeaderValue::from_static("2026-07-28"),
        );
        let out = outgoing(&inbound, "exchanged", "req-1").unwrap();
        let names: Vec<_> = out.keys().map(|name| name.as_str().to_owned()).collect();
        for kept in [
            "authorization",
            "content-type",
            "mcp-method",
            "mcp-protocol-version",
            "x-request-id",
        ] {
            assert!(
                names.iter().any(|name| name == kept),
                "{kept} missing in {names:?}"
            );
        }
        assert_eq!(names.len(), 5, "{names:?}");
        assert_eq!(out[header::AUTHORIZATION], "Bearer exchanged");
        assert!(out[header::AUTHORIZATION].is_sensitive());
    }

    #[test]
    fn natures_are_read_from_a_tools_list_answer() {
        let answer = json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": [
            {"name": "search_logs", "annotations": {"readOnlyHint": true}},
            {"name": "resolve_incident", "annotations": {"readOnlyHint": false, "destructiveHint": true}},
            {"name": "bare"},
            {"annotations": {}}
        ]}});
        let natures = natures_of(&answer).unwrap();
        assert_eq!(natures.len(), 3);
        assert!(natures["search_logs"].read_only);
        assert!(natures["resolve_incident"].destructive);
        assert!(!natures["bare"].read_only);
        assert!(natures_of(&json!({"error": {}})).is_none());
    }

    #[test]
    fn the_endpoint_is_the_org_scoped_mcp_path() {
        let upstream = Upstream::new(
            reqwest::Client::new(),
            Url::parse("http://mcp:8081/").unwrap(),
        );
        let org = Uuid::from_u128(1);
        assert_eq!(
            upstream.endpoint(org).as_str(),
            "http://mcp:8081/orgs/00000000-0000-0000-0000-000000000001/mcp"
        );
    }

    #[test]
    fn remembered_natures_are_found_by_tool() {
        let upstream = Upstream::new(
            reqwest::Client::new(),
            Url::parse("http://mcp:8081").unwrap(),
        );
        assert!(!upstream.is_warm());
        assert_eq!(upstream.nature_of("search_logs"), None);
        upstream.remember(HashMap::from([(
            "search_logs".to_owned(),
            Nature {
                read_only: true,
                destructive: false,
            },
        )]));
        assert!(upstream.is_warm());
        assert!(upstream.nature_of("search_logs").unwrap().read_only);
        assert_eq!(upstream.nature_of("nope"), None);
    }
}
