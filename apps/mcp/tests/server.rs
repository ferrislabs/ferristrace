use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use ferristrace_mcp::{ApiClient, MAX_BYTES, MAX_ROWS, TOOL_NAMES, router};
use rmcp::{
    ClientHandler, ClientLifecycleMode, ClientServiceExt,
    model::{CallToolRequestParams, CallToolResult, ProtocolVersion},
    service::{RoleClient, RunningService},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};

const ORG: &str = "11111111-1111-4111-8111-111111111111";
const OTHER_ORG: &str = "99999999-9999-4999-8999-999999999999";
const INCIDENT: &str = "22222222-2222-4222-8222-222222222222";
const PAGE: &str = "33333333-3333-4333-8333-333333333333";
const SCHEDULE: &str = "44444444-4444-4444-8444-444444444444";
const TOKEN: &str = "ey.exchanged-token_v1";

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    query: HashMap<String, Vec<String>>,
    authorization: String,
    body: Value,
}

impl Seen {
    fn param(&self, key: &str) -> &str {
        self.query
            .get(key)
            .and_then(|values| values.first())
            .map_or("", String::as_str)
    }

    fn millis(&self, key: &str) -> u64 {
        self.param(key).parse().unwrap()
    }
}

#[derive(Clone, Default)]
struct Fake {
    log: Arc<Mutex<Vec<Seen>>>,
    replies: Arc<Mutex<HashMap<String, (u16, String)>>>,
}

impl Fake {
    fn json(&self, method: &str, path: &str, status: u16, body: &Value) {
        self.text(method, path, status, &body.to_string());
    }

    fn text(&self, method: &str, path: &str, status: u16, body: &str) {
        self.replies
            .lock()
            .unwrap()
            .insert(format!("{method} {path}"), (status, body.to_owned()));
    }

    fn seen(&self) -> Vec<Seen> {
        self.log.lock().unwrap().clone()
    }

    fn only(&self) -> Seen {
        let seen = self.seen();
        assert_eq!(seen.len(), 1, "expected one API call, saw {seen:?}");
        seen[0].clone()
    }
}

async fn record(State(fake): State<Fake>, request: Request) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let mut query: HashMap<String, Vec<String>> = HashMap::new();
    for pair in request.uri().query().unwrap_or_default().split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            query
                .entry(key.to_owned())
                .or_default()
                .push(value.replace('+', " "));
        }
    }
    let authorization = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = axum::body::to_bytes(request.into_body(), 1 << 20)
        .await
        .unwrap();
    fake.log.lock().unwrap().push(Seen {
        method: method.clone(),
        path: path.clone(),
        query,
        authorization,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    });
    let reply = fake
        .replies
        .lock()
        .unwrap()
        .get(&format!("{method} {path}"))
        .cloned();
    match reply {
        Some((status, body)) => (
            StatusCode::from_u16(status).unwrap(),
            [("content-type", "application/json")],
            body,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "no such route").into_response(),
    }
}

#[derive(Clone, Default)]
struct Probe;

impl ClientHandler for Probe {}

struct Harness {
    fake: Fake,
    mcp: String,
    client: RunningService<RoleClient, Probe>,
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

async fn connect(mcp: &str, org: &str, token: &str) -> RunningService<RoleClient, Probe> {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("{mcp}/orgs/{org}/mcp"))
            .auth_header(token),
    );
    Probe
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap()
}

async fn harness_with(fake: Fake, api: String) -> Harness {
    let mcp = serve(router(ApiClient::new(&api).unwrap())).await;
    let client = connect(&mcp, ORG, TOKEN).await;
    Harness { fake, mcp, client }
}

async fn harness() -> Harness {
    let fake = Fake::default();
    let api = serve(Router::new().fallback(record).with_state(fake.clone())).await;
    harness_with(fake, api).await
}

impl Harness {
    async fn call(&self, tool: &'static str, arguments: &Value) -> CallToolResult {
        let params = CallToolRequestParams::new(tool)
            .with_arguments(arguments.as_object().cloned().unwrap_or_default());
        self.client.call_tool(params).await.unwrap()
    }
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .map(|block| {
            serde_json::to_value(block).unwrap()["text"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

fn answer(result: &CallToolResult) -> String {
    assert_ne!(result.is_error, Some(true), "tool error: {}", text(result));
    text(result)
}

fn api(tail: &str) -> String {
    format!("/api/orgs/{ORG}/{tail}")
}

#[tokio::test]
async fn tools_list_names_the_fifteen_tools_with_honest_annotations() {
    let h = harness().await;
    let tools = h.client.list_all_tools().await.unwrap();
    let mut names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    names.sort_unstable();
    let mut expected = TOOL_NAMES.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    assert_eq!(names.len(), 15);

    let hints = |name: &str| {
        let tool = tools.iter().find(|tool| tool.name == name).unwrap();
        let annotations = tool.annotations.clone().unwrap();
        (annotations.read_only_hint, annotations.destructive_hint)
    };
    for read in &TOOL_NAMES[..12] {
        assert_eq!(hints(read).0, Some(true), "{read}");
    }
    assert_eq!(hints("comment_incident"), (Some(false), Some(false)));
    assert_eq!(hints("acknowledge_page"), (Some(false), Some(false)));
    assert_eq!(hints("resolve_incident"), (Some(false), Some(true)));

    for tool in &tools {
        assert!(
            tool.description
                .as_deref()
                .is_some_and(|text| !text.is_empty()),
            "{}",
            tool.name
        );
        assert_eq!(tool.input_schema.get("type"), Some(&json!("object")));
        let properties = tool.input_schema["properties"].as_object().unwrap();
        for (argument, schema) in properties {
            assert!(
                schema.get("description").is_some(),
                "{}.{argument} has no description",
                tool.name
            );
        }
    }
    let logs = tools
        .iter()
        .find(|tool| tool.name == "search_logs")
        .unwrap();
    let fields: Vec<&String> = logs.input_schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    for field in [
        "service",
        "severity",
        "text",
        "namespace",
        "pod",
        "window",
        "limit",
    ] {
        assert!(fields.iter().any(|name| *name == field), "{field}");
    }
}

#[tokio::test]
async fn list_services_renders_a_table_from_the_organisations_route() {
    let h = harness().await;
    h.fake.json(
        "GET",
        &api("services"),
        200,
        &json!({"services": [{
            "name": "checkout", "requests": 100, "errors": 5, "per_second": 0.5,
            "error_rate": 0.05, "p50_us": 900, "p95_us": 20_000, "p99_us": 90_000,
            "last_seen_ms": 1_700_000_000_000_u64
        }]}),
    );
    let result = h.call("list_services", &json!({})).await;
    let out = answer(&result);
    assert!(out.starts_with("1 service seen in the last 1h\n"), "{out}");
    assert!(out.contains("service | requests | errors"));
    assert!(out.contains("checkout | 100 | 5 | 5.00%"), "{out}");

    let seen = h.fake.only();
    assert_eq!(seen.method, "GET");
    assert_eq!(seen.path, api("services"));
    assert_eq!(seen.millis("to") - seen.millis("from"), 3_600_000);
}

#[tokio::test]
async fn the_window_argument_sets_the_span_and_a_bad_one_never_reaches_the_api() {
    let h = harness().await;
    h.fake
        .json("GET", &api("services"), 200, &json!({"services": []}));
    answer(&h.call("list_services", &json!({"window": "7d"})).await);
    let seen = h.fake.only();
    assert_eq!(seen.millis("to") - seen.millis("from"), 7 * 86_400_000);

    let refused = h.call("list_services", &json!({"window": "3w"})).await;
    assert_eq!(refused.is_error, Some(true));
    assert!(text(&refused).contains("15m, 1h, 24h or 7d"));
    assert_eq!(h.fake.seen().len(), 1);
}

#[tokio::test]
async fn the_bearer_is_forwarded_unchanged_and_the_organisation_comes_from_the_path() {
    let h = harness().await;
    for org in [ORG, OTHER_ORG] {
        h.fake.json(
            "GET",
            &format!("/api/orgs/{org}/alerts"),
            200,
            &json!({"alerts": []}),
        );
    }
    answer(&h.call("list_alerts", &json!({})).await);
    let second = connect(&h.mcp, OTHER_ORG, "another.token").await;
    let result = second
        .call_tool(CallToolRequestParams::new("list_alerts"))
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true));

    let seen = h.fake.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].path, format!("/api/orgs/{ORG}/alerts"));
    assert_eq!(seen[0].authorization, format!("Bearer {TOKEN}"));
    assert_eq!(seen[1].path, format!("/api/orgs/{OTHER_ORG}/alerts"));
    assert_eq!(seen[1].authorization, "Bearer another.token");
}

#[tokio::test]
async fn a_request_without_a_bearer_is_refused_with_401_before_any_tool_runs() {
    let h = harness().await;
    let http = reqwest::Client::new();
    let url = format!("{}/orgs/{ORG}/mcp", h.mcp);
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "list_alerts", "arguments": {}}});
    for authorization in [None, Some("Basic abc"), Some("Bearer "), Some("Bearer a b")] {
        let mut request = http
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(value) = authorization {
            request = request.header("authorization", value);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 401, "{authorization:?}");
        assert!(response.headers().contains_key("www-authenticate"));
    }
    assert!(h.fake.seen().is_empty());
}

#[tokio::test]
async fn a_path_that_is_not_an_organisation_is_not_found_and_healthz_needs_no_token() {
    let h = harness().await;
    let http = reqwest::Client::new();
    let missing = http
        .post(format!("{}/orgs/not-a-uuid/mcp", h.mcp))
        .header("authorization", format!("Bearer {TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    let health = http.get(format!("{}/healthz", h.mcp)).send().await.unwrap();
    assert_eq!(health.status(), 200);
    assert!(h.fake.seen().is_empty());
}

#[tokio::test]
async fn a_403_from_the_api_becomes_a_tool_error_with_its_message() {
    let h = harness().await;
    h.fake.text(
        "GET",
        &api("logs"),
        403,
        "this agent holds no scope that allows this",
    );
    let result = h.call("search_logs", &json!({"service": "checkout"})).await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        text(&result),
        "FerrisTrace API 403: this agent holds no scope that allows this"
    );
}

#[tokio::test]
async fn a_json_error_body_and_a_missing_route_both_become_tool_errors() {
    let h = harness().await;
    h.fake.json(
        "GET",
        &api("incidents"),
        503,
        &json!({"message": "the incidents could not be read"}),
    );
    let down = h.call("list_incidents", &json!({})).await;
    assert_eq!(down.is_error, Some(true));
    assert_eq!(
        text(&down),
        "FerrisTrace API 503: the incidents could not be read"
    );
    let missing = h.call("list_alerts", &json!({})).await;
    assert_eq!(missing.is_error, Some(true));
    assert_eq!(text(&missing), "FerrisTrace API 404: no such route");
}

#[tokio::test]
async fn an_unreachable_api_is_a_tool_error_not_a_transport_error() {
    let dead = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let h = harness_with(Fake::default(), dead).await;
    let result = h.call("list_services", &json!({})).await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(text(&result), "the FerrisTrace API is unreachable");
}

#[tokio::test]
async fn a_long_answer_is_cut_at_the_row_cap_and_says_so() {
    let h = harness().await;
    let records: Vec<Value> = (0..300)
        .map(|i| {
            json!({"time_ns": 1_700_000_000_000_000_000_u64 + i, "severity": "error",
                "service": "checkout", "body": format!("line {i}"), "trace_id": null})
        })
        .collect();
    h.fake.json(
        "GET",
        &api("logs"),
        200,
        &json!({"records": records, "next": "1.2"}),
    );
    let result = h.call("search_logs", &json!({"limit": 200})).await;
    let out = answer(&result);
    let rows = out.lines().filter(|line| line.contains("| line ")).count();
    assert_eq!(rows, MAX_ROWS);
    assert!(
        out.contains("[truncated: showing 200 of 300 rows"),
        "{}",
        &out[out.len().saturating_sub(200)..]
    );
    assert!(out.len() <= MAX_BYTES);
}

#[tokio::test]
async fn a_fat_answer_is_cut_at_the_byte_cap() {
    let h = harness().await;
    let fat = "y".repeat(290);
    let spans: Vec<Value> = (0..200)
        .map(|i| {
            json!({"span_id": format!("{i}"), "parent_span_id": null, "service": fat,
                "name": fat, "status": "error", "start_offset_us": i, "duration_us": 1,
                "attributes": {"exception.message": fat}, "events": []})
        })
        .collect();
    let trace_id = "ab".repeat(16);
    h.fake.json(
        "GET",
        &api(&format!("traces/{trace_id}")),
        200,
        &json!({"trace_id": trace_id, "started_at_ms": 0, "duration_us": 9, "spans": spans}),
    );
    let result = h.call("get_trace", &json!({"trace_id": trace_id})).await;
    let out = answer(&result);
    assert!(out.len() <= MAX_BYTES, "{}", out.len());
    assert!(out.contains("[truncated: showing"));
    assert!(out.contains("of 200 rows"));
}

#[tokio::test]
async fn search_logs_sends_every_filter_to_the_logs_route() {
    let h = harness().await;
    h.fake.json(
        "GET",
        &api("logs"),
        200,
        &json!({"records": [], "next": null}),
    );
    answer(
        &h.call(
            "search_logs",
            &json!({"service": "checkout", "severity": "ERROR", "text": "timeout",
                "namespace": "shop", "pod": "cart-1", "window": "15m", "limit": 25}),
        )
        .await,
    );
    let seen = h.fake.only();
    assert_eq!(seen.param("service"), "checkout");
    assert_eq!(seen.param("severity"), "error");
    assert_eq!(seen.param("q"), "timeout");
    assert_eq!(seen.param("namespace"), "shop");
    assert_eq!(seen.param("pod"), "cart-1");
    assert_eq!(seen.param("limit"), "25");
    assert_eq!(seen.millis("to") - seen.millis("from"), 900_000);

    let refused = h.call("search_logs", &json!({"severity": "loud"})).await;
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(h.fake.seen().len(), 1);
}

#[tokio::test]
async fn search_traces_and_get_trace_use_the_trace_routes() {
    let h = harness().await;
    h.fake
        .json("GET", &api("traces"), 200, &json!({"traces": []}));
    answer(
        &h.call(
            "search_traces",
            &json!({"service": "checkout", "errors_only": true, "min_duration_ms": 250}),
        )
        .await,
    );
    let seen = h.fake.only();
    assert_eq!(seen.param("service"), "checkout");
    assert_eq!(seen.param("errors"), "true");
    assert_eq!(seen.param("at_least_us"), "250000");
    assert_eq!(seen.param("limit"), "20");

    let refused = h.call("get_trace", &json!({"trace_id": "../x"})).await;
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(h.fake.seen().len(), 1);
}

#[tokio::test]
async fn service_health_makes_two_calls_and_ranks_failing_operations() {
    let h = harness().await;
    h.fake.json(
        "GET",
        &api("services"),
        200,
        &json!({"services": [{"name": "api", "requests": 10, "errors": 2, "per_second": 1.0,
            "error_rate": 0.2, "p50_us": 1, "p95_us": 2, "p99_us": 3, "last_seen_ms": 0}]}),
    );
    h.fake.json(
        "GET",
        &api("services/api/operations"),
        200,
        &json!({"operations": [{"name": "POST /pay", "requests": 4, "errors": 2,
            "p95_ns": 1000, "p99_ns": 2000}]}),
    );
    let out = answer(&h.call("service_health", &json!({"service": "api"})).await);
    assert!(out.contains("- errors: 2 (20.00%)"), "{out}");
    assert!(out.contains("POST /pay | 4 | 2"));
    let paths: Vec<String> = h.fake.seen().into_iter().map(|seen| seen.path).collect();
    assert_eq!(paths, [api("services"), api("services/api/operations")]);
}

#[tokio::test]
async fn query_metric_groups_by_service_when_a_service_is_named() {
    let h = harness().await;
    h.fake.json(
        "GET",
        &api("metrics/http.server.duration/series"),
        200,
        &json!({"step_ns": 1, "series": [
            {"labels": {"service.name": "api"}, "points": [[1, 2.0], [2, 4.0]]},
            {"labels": {"service.name": "web"}, "points": [[1, 7.0]]}], "exemplars": []}),
    );
    let out = answer(
        &h.call(
            "query_metric",
            &json!({"name": "http.server.duration", "aggregation": "p95", "service": "api"}),
        )
        .await,
    );
    assert!(
        out.contains("service.name=api | 2 | 4 | 2 | 4 | 3"),
        "{out}"
    );
    assert!(!out.contains("web"));
    let seen = h.fake.only();
    assert_eq!(seen.param("agg"), "p95");
    assert_eq!(seen.param("group_by"), "service.name");
}

#[tokio::test]
async fn cluster_alerts_and_incidents_read_their_routes() {
    let h = harness().await;
    h.fake.json(
        "GET",
        &api("cluster"),
        200,
        &json!({"clusters": ["prod"], "namespaces": ["shop"], "nodes": [], "workloads": [],
            "pods": [], "seen_ms": null}),
    );
    h.fake.json(
        "GET",
        &api("alerts"),
        200,
        &json!({"alerts": [
            {"id": "a1", "name": "High errors", "severity": "critical", "enabled": true,
             "state": {"status": "firing", "value": 7.0, "since_ms": 0}},
            {"id": "a2", "name": "Disk", "severity": "warning", "enabled": true,
             "state": {"status": "ok", "value": 1.0, "since_ms": 0}}]}),
    );
    h.fake.json(
        "GET",
        &api("incidents"),
        200,
        &json!({"incidents": [{"id": INCIDENT, "title": "Checkout down", "severity": "sev1",
            "status": "investigating", "services": ["checkout"], "opened_ms": 0, "owner": null}]}),
    );
    answer(
        &h.call(
            "cluster_status",
            &json!({"cluster": "prod", "namespace": "shop"}),
        )
        .await,
    );
    let alerts = answer(&h.call("list_alerts", &json!({"state": "firing"})).await);
    assert!(alerts.contains("High errors"));
    assert!(!alerts.contains("Disk"));
    let incidents = answer(&h.call("list_incidents", &json!({"status": "all"})).await);
    assert!(incidents.contains(&format!(
        "{INCIDENT} | sev1 | investigating | Checkout down"
    )));

    let seen = h.fake.seen();
    assert_eq!(seen[0].param("cluster"), "prod");
    assert_eq!(seen[0].param("namespace"), "shop");
    assert_eq!(seen[2].param("status"), "all");
}

#[tokio::test]
async fn get_incident_and_alert_history_validate_their_ids() {
    let h = harness().await;
    h.fake.json(
        "GET",
        &api(&format!("incidents/{INCIDENT}")),
        200,
        &json!({"id": INCIDENT, "title": "Checkout down", "severity": "sev1",
            "status": "investigating", "owner": null, "opened_ms": 0, "resolved_ms": null,
            "services": [], "alert": null, "attachments": [], "timeline": []}),
    );
    let out = answer(
        &h.call("get_incident", &json!({"incident_id": INCIDENT}))
            .await,
    );
    assert!(out.starts_with(&format!("incident {INCIDENT}: Checkout down")));
    let bad = h
        .call("get_incident", &json!({"incident_id": "nope"}))
        .await;
    assert_eq!(text(&bad), "incident_id must be a UUID");
    let history = h.call("alert_history", &json!({"alert_id": "x"})).await;
    assert_eq!(history.is_error, Some(true));
    assert_eq!(h.fake.seen().len(), 1);
}

fn rota() -> Value {
    json!({"schedules": [
        {"id": SCHEDULE, "name": "Primary", "team": INCIDENT,
         "on_call": {"email": "ada@example.com", "name": "Ada"},
         "until_ms": 1_700_000_000_000_u64},
        {"id": PAGE, "name": "Backup", "team": INCIDENT, "on_call": null, "until_ms": null}
    ]})
}

#[tokio::test]
async fn who_is_on_call_lists_every_schedule_with_one_call() {
    let h = harness().await;
    h.fake.json("GET", &api("oncall/now"), 200, &rota());
    let out = answer(&h.call("who_is_on_call", &json!({})).await);
    assert!(
        out.contains("Primary | Ada | 2023-11-14T22:13:20Z"),
        "{out}"
    );
    assert!(out.contains("Backup | nobody | -"), "{out}");
    let seen = h.fake.only();
    assert_eq!(seen.path, api("oncall/now"));
    assert_eq!(seen.param("email"), "");
}

#[tokio::test]
async fn who_is_on_call_filters_by_name_or_id_and_refuses_an_unknown_schedule() {
    let h = harness().await;
    h.fake.json("GET", &api("oncall/now"), 200, &rota());
    let by_name = answer(
        &h.call("who_is_on_call", &json!({"schedule": "primary"}))
            .await,
    );
    assert!(
        by_name.contains("Primary") && !by_name.contains("Backup"),
        "{by_name}"
    );
    let by_id = answer(&h.call("who_is_on_call", &json!({"schedule": PAGE})).await);
    assert!(
        by_id.contains("Backup") && !by_id.contains("Primary"),
        "{by_id}"
    );
    let unknown = h
        .call("who_is_on_call", &json!({"schedule": "ghost"}))
        .await;
    assert_eq!(unknown.is_error, Some(true));
    assert!(text(&unknown).contains("\"ghost\""));
}

#[tokio::test]
async fn comment_resolve_and_acknowledge_write_through_the_documented_routes() {
    let h = harness().await;
    h.fake.json(
        "POST",
        &api(&format!("incidents/{INCIDENT}/comments")),
        201,
        &json!({"at_ms": 1_700_000_000_000_u64, "kind": "comment"}),
    );
    h.fake.json(
        "PATCH",
        &api(&format!("incidents/{INCIDENT}")),
        200,
        &json!({"id": INCIDENT, "title": "Checkout down", "status": "resolved"}),
    );
    h.fake.json(
        "POST",
        &api(&format!("pages/{PAGE}/ack")),
        200,
        &json!({"id": PAGE, "state": "acknowledged", "headline": "High errors"}),
    );

    let commented = answer(
        &h.call(
            "comment_incident",
            &json!({"incident_id": INCIDENT, "text": "rolled back the deploy"}),
        )
        .await,
    );
    assert!(commented.starts_with(&format!("comment added to incident {INCIDENT}")));
    let resolved = answer(
        &h.call("resolve_incident", &json!({"incident_id": INCIDENT}))
            .await,
    );
    assert_eq!(
        resolved.trim(),
        format!("incident {INCIDENT} is resolved: Checkout down")
    );
    let acked = answer(&h.call("acknowledge_page", &json!({"page_id": PAGE})).await);
    assert_eq!(
        acked.trim(),
        format!("page {PAGE} is acknowledged: High errors")
    );

    let seen = h.fake.seen();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(
        seen[0].body,
        json!({"body": "rolled back the deploy", "public": false})
    );
    assert_eq!(seen[1].method, "PATCH");
    assert_eq!(seen[1].body, json!({"status": "resolved"}));
    assert_eq!(seen[2].method, "POST");
    assert_eq!(seen[2].path, api(&format!("pages/{PAGE}/ack")));
    for call in &seen {
        assert_eq!(call.authorization, format!("Bearer {TOKEN}"));
    }
}

#[tokio::test]
async fn a_write_the_api_refuses_comes_back_as_a_tool_error() {
    let h = harness().await;
    h.fake.text(
        "PATCH",
        &api(&format!("incidents/{INCIDENT}")),
        403,
        "this agent holds no scope that allows this",
    );
    let result = h
        .call("resolve_incident", &json!({"incident_id": INCIDENT}))
        .await;
    assert_eq!(result.is_error, Some(true));
    assert!(text(&result).contains("403"));

    let empty = h
        .call(
            "comment_incident",
            &json!({"incident_id": INCIDENT, "text": "  "}),
        )
        .await;
    assert_eq!(text(&empty), "text is required");
    assert_eq!(h.fake.seen().len(), 1);
}
