use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::export::Queue;

pub const KIND: &str = "mcp_decision";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DecisionRecord {
    pub kind: &'static str,
    pub time: String,
    pub request_id: String,
    pub client_request_id: Option<String>,
    pub method: String,
    pub org: String,
    pub tool: Option<String>,
    pub subject_type: &'static str,
    pub subject: String,
    pub groups: Vec<String>,
    pub agent: String,
    pub delegated: bool,
    pub act: Option<Value>,
    pub target: Map<String, Value>,
    pub arguments: Map<String, Value>,
    pub decision: &'static str,
    pub policies: Vec<String>,
    pub reason: Option<&'static str>,
}

pub const MAX_TEXT_CHARS: usize = 256;
pub const MAX_RECORD_BYTES: usize = 4096;
const MAX_LIST: usize = 32;

fn clip(text: &mut String, limit: usize) {
    if let Some((index, _)) = text.char_indices().nth(limit) {
        text.truncate(index);
    }
}

fn clipped(map: &Map<String, Value>) -> Map<String, Value> {
    map.iter()
        .map(|(key, value)| {
            let mut key = key.clone();
            clip(&mut key, MAX_TEXT_CHARS);
            let value = match value {
                Value::String(text) => {
                    let mut text = text.clone();
                    clip(&mut text, MAX_TEXT_CHARS);
                    Value::String(text)
                }
                Value::Array(items) => Value::Array(
                    items
                        .iter()
                        .take(MAX_LIST)
                        .map(|item| match item {
                            Value::String(text) => {
                                let mut text = text.clone();
                                clip(&mut text, MAX_TEXT_CHARS);
                                Value::String(text)
                            }
                            other => other.clone(),
                        })
                        .collect(),
                ),
                other => other.clone(),
            };
            (key, value)
        })
        .collect()
}

fn clip_all(items: &mut Vec<String>) {
    items.truncate(MAX_LIST);
    for item in items.iter_mut() {
        clip(item, MAX_TEXT_CHARS);
    }
}

impl DecisionRecord {
    pub fn size(&self) -> usize {
        serde_json::to_vec(self).map_or(MAX_RECORD_BYTES, |bytes| bytes.len())
    }

    #[must_use]
    pub fn bounded(mut self) -> Self {
        for text in [
            &mut self.method,
            &mut self.org,
            &mut self.subject,
            &mut self.agent,
        ] {
            clip(text, MAX_TEXT_CHARS);
        }
        if let Some(tool) = &mut self.tool {
            clip(tool, MAX_TEXT_CHARS);
        }
        if let Some(id) = &mut self.client_request_id {
            clip(id, 128);
        }
        clip_all(&mut self.groups);
        clip_all(&mut self.policies);
        self.target = clipped(&self.target);
        self.arguments = clipped(&self.arguments);
        if let Some(act) = &self.act
            && serde_json::to_string(act).map_or(true, |text| text.len() > MAX_RECORD_BYTES / 4)
        {
            self.act = Some(json!({"truncated": true}));
        }
        if self.size() > MAX_RECORD_BYTES {
            self.arguments = truncated();
        }
        if self.size() > MAX_RECORD_BYTES {
            self.target = truncated();
        }
        if self.size() > MAX_RECORD_BYTES {
            self.groups.clear();
            self.policies.clear();
        }
        self
    }
}

fn truncated() -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("truncated".to_owned(), Value::Bool(true));
    map
}

pub trait DecisionSink: Send + Sync + 'static {
    fn record(&self, record: DecisionRecord);
}

#[derive(Debug, Default)]
pub struct Recorder {
    queue: Option<Queue>,
}

impl Recorder {
    pub fn logging() -> Self {
        Self { queue: None }
    }

    pub fn exporting(queue: Queue) -> Self {
        Self { queue: Some(queue) }
    }
}

fn json_of<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

impl DecisionSink for Recorder {
    fn record(&self, record: DecisionRecord) {
        tracing::info!(
            target: "mcp_decision",
            kind = record.kind,
            request_id = %record.request_id,
            client_request_id = record.client_request_id.as_deref().unwrap_or(""),
            method = %record.method,
            org = %record.org,
            tool = record.tool.as_deref().unwrap_or(""),
            subject_type = record.subject_type,
            subject = %record.subject,
            groups = %json_of(&record.groups),
            agent = %record.agent,
            delegated = record.delegated,
            act = %record.act.as_ref().map(json_of).unwrap_or_default(),
            target = %json_of(&record.target),
            arguments = %json_of(&record.arguments),
            decision = record.decision,
            policies = %json_of(&record.policies),
            reason = record.reason.unwrap_or(""),
            "mcp decision"
        );
        if let Some(queue) = &self.queue {
            queue.offer(record);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use serde_json::json;
    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buffer {
        type Writer = Buffer;

        fn make_writer(&'a self) -> Buffer {
            self.clone()
        }
    }

    fn record() -> DecisionRecord {
        let mut target = Map::new();
        target.insert("service".to_owned(), json!("checkout"));
        let mut arguments = Map::new();
        arguments.insert("limit".to_owned(), json!(5));
        DecisionRecord {
            kind: KIND,
            time: "2026-11-03T09:05:00Z".to_owned(),
            request_id: "req-9".to_owned(),
            client_request_id: Some("theirs".to_owned()),
            method: "tools/call".to_owned(),
            org: "org-1".to_owned(),
            tool: Some("search_logs".to_owned()),
            subject_type: "User",
            subject: "alice-sub".to_owned(),
            groups: vec!["Dev".to_owned()],
            agent: "claude-code".to_owned(),
            delegated: true,
            act: Some(json!({"client_id": "gateway"})),
            target,
            arguments,
            decision: "deny",
            policies: vec!["p1".to_owned()],
            reason: Some("no_permitting_policy"),
        }
    }

    #[test]
    fn one_structured_event_carries_every_field_of_the_decision() {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buffer.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            Recorder::logging().record(record());
        });
        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output.lines().count(), 1, "{output}");
        for expected in [
            "mcp_decision",
            "kind=\"mcp_decision\"",
            "subject=alice-sub",
            "groups=[\"Dev\"]",
            "act={\"client_id\":\"gateway\"}",
            "org=org-1",
            "tool=\"search_logs\"",
            "target={\"service\":\"checkout\"}",
            "arguments={\"limit\":5}",
            "decision=\"deny\"",
            "policies=[\"p1\"]",
            "reason=\"no_permitting_policy\"",
            "request_id=req-9",
            "client_request_id=\"theirs\"",
        ] {
            assert!(output.contains(expected), "{expected} missing in {output}");
        }
    }

    #[test]
    fn long_strings_are_cut_and_the_record_stays_small() {
        let mut long = record();
        long.arguments
            .insert("service".to_owned(), json!("é".repeat(1000)));
        long.groups = (0..100).map(|index| format!("group-{index}")).collect();
        long.policies = vec!["p".repeat(5000)];
        let bounded = long.bounded();
        assert_eq!(
            bounded.arguments["service"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            256
        );
        assert_eq!(bounded.groups.len(), 32);
        assert_eq!(bounded.policies[0].chars().count(), 256);
        assert!(bounded.size() <= MAX_RECORD_BYTES);
    }

    #[test]
    fn a_record_over_the_cap_drops_its_arguments_then_its_target() {
        let mut huge = record();
        for index in 0..400 {
            huge.arguments
                .insert(format!("argument-{index}"), json!("x".repeat(200)));
        }
        let bounded = huge.bounded();
        assert_eq!(bounded.arguments["truncated"], true);
        assert!(bounded.size() <= MAX_RECORD_BYTES);
        let mut wide = record();
        for index in 0..400 {
            wide.target
                .insert(format!("t-{index}"), json!("x".repeat(200)));
        }
        let bounded = wide.bounded();
        assert_eq!(bounded.target["truncated"], true);
        assert!(bounded.size() <= MAX_RECORD_BYTES);
    }

    #[test]
    fn a_small_record_is_left_alone() {
        let small = record();
        assert_eq!(small.clone().bounded(), small);
    }

    #[test]
    fn the_record_serialises_with_the_documented_names() {
        let value = serde_json::to_value(record()).unwrap();
        let object = value.as_object().unwrap();
        for key in [
            "kind",
            "subject",
            "groups",
            "act",
            "org",
            "tool",
            "target",
            "arguments",
            "decision",
            "policies",
            "reason",
            "request_id",
        ] {
            assert!(object.contains_key(key), "{key}");
        }
    }
}
