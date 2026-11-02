use super::value_objects::{CallCursor, Cost, ModelPattern, PriceId, Provider, Rates};
use crate::telemetry::{SpanId, TraceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub time_unix_nanos: u64,
    pub trace: TraceId,
    pub span: SpanId,
    pub service: Box<str>,
    pub provider: Box<str>,
    pub operation: Box<str>,
    pub request_model: Box<str>,
    pub response_model: Box<str>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub duration_nanos: u64,
    pub error: bool,
    pub feature: Box<str>,
    pub customer: Box<str>,
    pub user: Box<str>,
    pub prompt_version: Box<str>,
    pub environment: Box<str>,
}

impl Call {
    pub fn model(&self) -> &str {
        if self.response_model.is_empty() {
            &self.request_model
        } else {
            &self.response_model
        }
    }

    pub fn time_ms(&self) -> u64 {
        self.time_unix_nanos / 1_000_000
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Price {
    pub id: PriceId,
    pub provider: Provider,
    pub model: ModelPattern,
    pub rates: Rates,
    pub effective_from_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedCall {
    pub call: Call,
    pub cost: Option<Cost>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallPage {
    pub calls: Vec<PricedCall>,
    pub next: Option<CallCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenModel {
    pub provider: Box<str>,
    pub request_model: Box<str>,
    pub response_model: Box<str>,
    pub calls: u64,
    pub last_unix_nanos: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpriced {
    pub provider: Box<str>,
    pub model: Box<str>,
    pub calls: u64,
    pub last_unix_nanos: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketTotals {
    pub key: Box<str>,
    pub provider: Box<str>,
    pub request_model: Box<str>,
    pub response_model: Box<str>,
    pub slot: u64,
    pub calls: u64,
    pub errors: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupLatency {
    pub key: Box<str>,
    pub p50_nanos: u64,
    pub p95_nanos: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptMarker {
    pub time_unix_nanos: u64,
    pub feature: Box<str>,
    pub prompt_version: Box<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpendRead {
    pub buckets: Vec<BucketTotals>,
    pub latencies: Vec<GroupLatency>,
    pub markers: Vec<PromptMarker>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendGroup {
    pub key: Box<str>,
    pub cost: Cost,
    pub unpriced: u64,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub p50_nanos: u64,
    pub p95_nanos: u64,
    pub errors: u64,
    pub points: Vec<(u64, Cost)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spend {
    pub step_nanos: u64,
    pub groups: Vec<SpendGroup>,
    pub markers: Vec<PromptMarker>,
}
