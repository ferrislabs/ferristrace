mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use ferristrace_mcp_gateway::Secret;
use ferristrace_mcp_gateway::decision::{DecisionRecord, DecisionSink, KIND, Recorder};
use ferristrace_mcp_gateway::export;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::common::v1::{KeyValue, any_value::Value};
use prost::Message;
use serde_json::{Map, json};

type Delivery = (String, String, Vec<u8>);

#[derive(Clone)]
struct Ingest {
    status: u16,
    received: Arc<Mutex<Vec<Delivery>>>,
}

async fn take(State(ingest): State<Ingest>, request: Request) -> impl IntoResponse {
    let authorization = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let bytes = axum::body::to_bytes(request.into_body(), 1 << 20)
        .await
        .unwrap();
    ingest
        .received
        .lock()
        .unwrap()
        .push((authorization, content_type, bytes.to_vec()));
    StatusCode::from_u16(ingest.status).unwrap()
}

async fn ingest(status: u16) -> (Ingest, String) {
    let state = Ingest {
        status,
        received: Arc::default(),
    };
    let url = support::serve(Router::new().fallback(take).with_state(state.clone())).await;
    (state, format!("{url}/v1/logs"))
}

fn record(decision: &'static str, reason: Option<&'static str>, tool: &str) -> DecisionRecord {
    let mut target = Map::new();
    target.insert("service".to_owned(), json!("checkout"));
    DecisionRecord {
        kind: KIND,
        time: "2026-11-03T09:05:00Z".to_owned(),
        request_id: "req-1".to_owned(),
        method: "tools/call".to_owned(),
        org: "11111111-1111-4111-8111-111111111111".to_owned(),
        tool: Some(tool.to_owned()),
        subject_type: "User",
        subject: "alice-sub".to_owned(),
        groups: vec!["Dev".to_owned(), "Support".to_owned()],
        agent: "claude-code".to_owned(),
        delegated: true,
        act: Some(json!({"client_id": "gateway"})),
        target,
        arguments: Map::new(),
        decision,
        policies: vec!["p1".to_owned()],
        reason,
    }
}

fn text<'a>(attributes: &'a [KeyValue], key: &str) -> &'a str {
    match attributes
        .iter()
        .find(|pair| pair.key == key)
        .and_then(|pair| pair.value.as_ref())
        .and_then(|value| value.value.as_ref())
    {
        Some(Value::StringValue(text)) => text,
        other => panic!("{key} is {other:?}"),
    }
}

async fn eventually(check: impl Fn() -> bool) {
    for _ in 0..200 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("never happened");
}

#[tokio::test]
async fn decisions_are_exported_as_otlp_logs_with_the_ingest_key() {
    let (state, endpoint) = ingest(200).await;
    let queue = export::spawn_with(
        reqwest::Client::new(),
        endpoint,
        Secret::new("ingest-key"),
        10,
        Duration::from_millis(50),
    );
    let recorder = Recorder::exporting(queue);
    recorder.record(record("deny", Some("pip_unavailable"), "resolve_incident"));
    recorder.record(record("permit", None, "search_logs"));
    eventually(|| {
        state
            .received
            .lock()
            .unwrap()
            .iter()
            .map(|(_, _, body)| ExportLogsServiceRequest::decode(body.as_slice()).unwrap())
            .map(|request| request.resource_logs[0].scope_logs[0].log_records.len())
            .sum::<usize>()
            == 2
    })
    .await;

    let received = state.received.lock().unwrap().clone();
    let (authorization, content_type, _) = &received[0];
    assert_eq!(authorization, "Bearer ingest-key");
    assert_eq!(content_type, "application/x-protobuf");
    let requests: Vec<ExportLogsServiceRequest> = received
        .iter()
        .map(|(_, _, body)| ExportLogsServiceRequest::decode(body.as_slice()).unwrap())
        .collect();
    let resource = requests[0].resource_logs[0].resource.as_ref().unwrap();
    assert_eq!(
        text(&resource.attributes, "service.name"),
        "ferristrace-mcp-gateway"
    );
    let records: Vec<_> = requests
        .iter()
        .flat_map(|request| request.resource_logs[0].scope_logs[0].log_records.iter())
        .collect();
    let denied = records[0];
    assert_eq!(denied.severity_text, "WARN");
    assert_eq!(denied.severity_number, 13);
    assert_eq!(denied.time_unix_nano, 1_793_696_700_000_000_000);
    assert_eq!(text(&denied.attributes, "kind"), "mcp_decision");
    assert_eq!(text(&denied.attributes, "decision"), "deny");
    assert_eq!(text(&denied.attributes, "reason"), "pip_unavailable");
    assert_eq!(text(&denied.attributes, "tool"), "resolve_incident");
    assert_eq!(text(&denied.attributes, "subject"), "alice-sub");
    assert_eq!(text(&denied.attributes, "request_id"), "req-1");
    assert_eq!(
        text(&denied.attributes, "organisation"),
        "11111111-1111-4111-8111-111111111111"
    );
    assert_eq!(
        text(&denied.attributes, "target"),
        r#"{"service":"checkout"}"#
    );
    assert_eq!(
        text(&denied.attributes, "act"),
        r#"{"client_id":"gateway"}"#
    );
    let groups = denied
        .attributes
        .iter()
        .find(|pair| pair.key == "groups")
        .unwrap();
    let Some(Value::ArrayValue(array)) = groups.value.as_ref().unwrap().value.as_ref() else {
        panic!("groups is not a list");
    };
    assert_eq!(array.values.len(), 2);
    let permitted = records[1];
    assert_eq!(permitted.severity_text, "INFO");
    assert!(permitted.attributes.iter().all(|pair| pair.key != "reason"));
}

#[tokio::test]
async fn a_refused_batch_is_counted_as_dropped_and_the_gateway_goes_on() {
    let (state, endpoint) = ingest(500).await;
    let queue = export::spawn_with(
        reqwest::Client::new(),
        endpoint,
        Secret::new("ingest-key"),
        10,
        Duration::from_millis(50),
    );
    queue.offer(record("permit", None, "a"));
    queue.offer(record("permit", None, "b"));
    eventually(|| queue.dropped() == 2).await;
    assert!(!state.received.lock().unwrap().is_empty());
    queue.offer(record("permit", None, "c"));
    eventually(|| queue.dropped() == 3).await;
}

#[tokio::test]
async fn an_unreachable_ingest_never_blocks_a_decision() {
    let queue = export::spawn_with(
        reqwest::Client::new(),
        format!("{}/v1/logs", support::closed_port_url()),
        Secret::new("k"),
        10,
        Duration::from_millis(20),
    );
    let started = std::time::Instant::now();
    for _ in 0..50 {
        queue.offer(record("permit", None, "a"));
    }
    assert!(started.elapsed() < Duration::from_millis(500));
    eventually(|| queue.dropped() >= 1).await;
}
