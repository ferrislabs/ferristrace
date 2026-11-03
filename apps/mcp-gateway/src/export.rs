use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::common::v1::{
    AnyValue, ArrayValue, InstrumentationScope, KeyValue, any_value::Value as Wire,
};
use opentelemetry_proto::tonic::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use opentelemetry_proto::tonic::resource::v1::Resource;
use prost::Message;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::decision::DecisionRecord;
use crate::secret::Secret;

pub const SERVICE_NAME: &str = "ferristrace-mcp-gateway";
const QUEUE_CAPACITY: usize = 1024;
const BATCH_SIZE: usize = 100;
const FLUSH_EVERY: Duration = Duration::from_secs(2);
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const SEVERITY_INFO: i32 = 9;
const SEVERITY_WARN: i32 = 13;

#[derive(Debug)]
pub struct Queue {
    sender: mpsc::Sender<DecisionRecord>,
    dropped: Arc<AtomicU64>,
}

impl Queue {
    pub fn bounded(capacity: usize) -> (Self, mpsc::Receiver<DecisionRecord>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (
            Self {
                sender,
                dropped: Arc::new(AtomicU64::new(0)),
            },
            receiver,
        )
    }

    pub fn offer(&self, record: DecisionRecord) {
        if self.sender.try_send(record).is_err() {
            let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::warn!(
                dropped_total = total,
                "a decision was not exported: the queue is full or closed"
            );
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.dropped)
    }
}

pub fn spawn(http: reqwest::Client, endpoint: String, key: Secret) -> Queue {
    spawn_with(http, endpoint, key, BATCH_SIZE, FLUSH_EVERY)
}

pub fn spawn_with(
    http: reqwest::Client,
    endpoint: String,
    key: Secret,
    batch_size: usize,
    flush_every: Duration,
) -> Queue {
    let (queue, receiver) = Queue::bounded(QUEUE_CAPACITY);
    let dropped = queue.counter();
    tokio::spawn(run(
        receiver,
        http,
        endpoint,
        key,
        dropped,
        batch_size,
        flush_every,
    ));
    queue
}

async fn run(
    mut receiver: mpsc::Receiver<DecisionRecord>,
    http: reqwest::Client,
    endpoint: String,
    key: Secret,
    dropped: Arc<AtomicU64>,
    batch_size: usize,
    flush_every: Duration,
) {
    while let Some(first) = receiver.recv().await {
        let mut batch = vec![first];
        let deadline = Instant::now() + flush_every;
        while batch.len() < batch_size {
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
                Ok(Some(record)) => batch.push(record),
                Ok(None) | Err(_) => break,
            }
        }
        if ship(&http, &endpoint, &key, &batch).await.is_err() {
            let lost = u64::try_from(batch.len()).unwrap_or(u64::MAX);
            let total = dropped.fetch_add(lost, Ordering::Relaxed) + lost;
            tracing::warn!(
                dropped_total = total,
                "a batch of decisions was not exported"
            );
        }
    }
}

async fn ship(
    http: &reqwest::Client,
    endpoint: &str,
    key: &Secret,
    batch: &[DecisionRecord],
) -> Result<(), ()> {
    let response = http
        .post(endpoint)
        .timeout(SEND_TIMEOUT)
        .bearer_auth(key.expose())
        .header("content-type", "application/x-protobuf")
        .body(encode(batch))
        .send()
        .await
        .map_err(|error| tracing::warn!(%error, "the ingest endpoint did not answer"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        tracing::warn!(status = %response.status(), "the ingest endpoint refused the decisions");
        Err(())
    }
}

fn text(value: &str) -> AnyValue {
    AnyValue {
        value: Some(Wire::StringValue(value.to_owned())),
    }
}

fn pair(key: &str, value: AnyValue) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(value),
        ..KeyValue::default()
    }
}

fn strings(values: &[String]) -> AnyValue {
    AnyValue {
        value: Some(Wire::ArrayValue(ArrayValue {
            values: values.iter().map(|value| text(value)).collect(),
        })),
    }
}

fn json(value: &impl serde::Serialize) -> AnyValue {
    text(&serde_json::to_string(value).unwrap_or_default())
}

fn log_of(record: &DecisionRecord) -> LogRecord {
    let nanos = record
        .time
        .parse::<jiff::Timestamp>()
        .ok()
        .and_then(|time| u64::try_from(time.as_nanosecond()).ok())
        .unwrap_or_default();
    let denied = record.decision != "permit";
    let mut attributes = vec![
        pair("kind", text(record.kind)),
        pair("request_id", text(&record.request_id)),
        pair("method", text(&record.method)),
        pair("organisation", text(&record.org)),
        pair("subject.type", text(record.subject_type)),
        pair("subject", text(&record.subject)),
        pair("groups", strings(&record.groups)),
        pair("agent", text(&record.agent)),
        pair(
            "delegated",
            AnyValue {
                value: Some(Wire::BoolValue(record.delegated)),
            },
        ),
        pair("target", json(&record.target)),
        pair("arguments", json(&record.arguments)),
        pair("decision", text(record.decision)),
        pair("policies", strings(&record.policies)),
    ];
    if let Some(tool) = &record.tool {
        attributes.push(pair("tool", text(tool)));
    }
    if let Some(act) = &record.act {
        attributes.push(pair("act", json(act)));
    }
    if let Some(reason) = record.reason {
        attributes.push(pair("reason", text(reason)));
    }
    LogRecord {
        time_unix_nano: nanos,
        observed_time_unix_nano: nanos,
        severity_number: if denied { SEVERITY_WARN } else { SEVERITY_INFO },
        severity_text: if denied { "WARN" } else { "INFO" }.to_owned(),
        body: Some(text(&format!(
            "mcp decision: {} {}",
            record.decision,
            record.tool.as_deref().unwrap_or(&record.method)
        ))),
        attributes,
        ..LogRecord::default()
    }
}

pub fn encode(batch: &[DecisionRecord]) -> Vec<u8> {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![pair("service.name", text(SERVICE_NAME))],
                ..Resource::default()
            }),
            scope_logs: vec![ScopeLogs {
                scope: Some(InstrumentationScope {
                    name: SERVICE_NAME.to_owned(),
                    ..InstrumentationScope::default()
                }),
                log_records: batch.iter().map(log_of).collect(),
                ..ScopeLogs::default()
            }],
            ..ResourceLogs::default()
        }],
    }
    .encode_to_vec()
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    fn record() -> DecisionRecord {
        DecisionRecord {
            kind: crate::decision::KIND,
            time: "2026-11-03T09:05:00Z".to_owned(),
            request_id: "r".to_owned(),
            method: "tools/call".to_owned(),
            org: "o".to_owned(),
            tool: None,
            subject_type: "Client",
            subject: "c".to_owned(),
            groups: Vec::new(),
            agent: "c".to_owned(),
            delegated: false,
            act: None,
            target: Map::new(),
            arguments: Map::new(),
            decision: "permit",
            policies: Vec::new(),
            reason: None,
        }
    }

    #[test]
    fn a_full_queue_drops_with_a_count_and_never_blocks() {
        let (queue, mut receiver) = Queue::bounded(2);
        for _ in 0..5 {
            queue.offer(record());
        }
        assert_eq!(queue.dropped(), 3);
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
        queue.offer(record());
        assert_eq!(queue.dropped(), 3);
    }

    #[test]
    fn a_closed_queue_counts_what_it_cannot_take() {
        let (queue, receiver) = Queue::bounded(2);
        drop(receiver);
        queue.offer(record());
        assert_eq!(queue.dropped(), 1);
    }

    #[test]
    fn an_empty_batch_still_encodes_and_a_missing_time_is_zero() {
        let mut undated = record();
        undated.time = "not a time".to_owned();
        let bytes = encode(&[undated]);
        let request = ExportLogsServiceRequest::decode(bytes.as_slice()).unwrap();
        assert_eq!(
            request.resource_logs[0].scope_logs[0].log_records[0].time_unix_nano,
            0
        );
        assert!(!encode(&[]).is_empty());
    }
}
