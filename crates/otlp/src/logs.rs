use ferristrace_domain::{InvalidId, LogRecord, Severity, SpanId, TraceId};
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, any_value::Value};
use opentelemetry_proto::tonic::logs::v1::{LogRecord as ProtoRecord, ResourceLogs};
use prost::Message;
use serde_json::{Map, Number, Value as Json};

use crate::attributes::{self, Attributes};
use crate::traces::{DecodeError, UNKNOWN_SERVICE};

const SERVICE_NAME: &str = "service.name";

pub fn decode_logs(payload: &[u8]) -> Result<Vec<LogRecord>, DecodeError> {
    logs_of(ExportLogsServiceRequest::decode(payload)?)
}

pub fn logs_of(request: ExportLogsServiceRequest) -> Result<Vec<LogRecord>, DecodeError> {
    let mut records = Vec::with_capacity(record_count(&request.resource_logs));
    for resource_logs in request.resource_logs {
        let pairs = resource_logs
            .resource
            .map(|resource| resource.attributes)
            .unwrap_or_default();
        let service =
            attributes::lookup(&pairs, SERVICE_NAME).unwrap_or_else(|| UNKNOWN_SERVICE.into());
        let resource = attributes::collect(pairs);
        for scope_logs in resource_logs.scope_logs {
            for record in scope_logs.log_records {
                records.push(convert(record, &service, &resource)?);
            }
        }
    }
    Ok(records)
}

fn record_count(resource_logs: &[ResourceLogs]) -> usize {
    resource_logs
        .iter()
        .flat_map(|resource| resource.scope_logs.iter())
        .map(|scope| scope.log_records.len())
        .sum()
}

fn convert(
    record: ProtoRecord,
    service: &str,
    resource: &Attributes,
) -> Result<LogRecord, DecodeError> {
    let ProtoRecord {
        time_unix_nano,
        observed_time_unix_nano,
        severity_number,
        severity_text,
        body,
        attributes: pairs,
        trace_id,
        span_id,
        ..
    } = record;

    let time = if time_unix_nano == 0 {
        observed_time_unix_nano
    } else {
        time_unix_nano
    };
    let observed = if observed_time_unix_nano == 0 {
        time
    } else {
        observed_time_unix_nano
    };

    Ok(LogRecord {
        time_unix_nanos: time,
        observed_unix_nanos: observed,
        severity: severity_of(severity_number, &severity_text),
        severity_number: u8::try_from(severity_number)
            .ok()
            .filter(|number| *number <= MOST_SEVERE)
            .unwrap_or_default(),
        service_name: service.into(),
        body: body_of(body),
        attributes: attributes::collect(pairs),
        resource: resource.clone(),
        trace_id: optional_id(&trace_id, |bytes| TraceId::try_from(bytes))?,
        span_id: optional_id(&span_id, |bytes| SpanId::try_from(bytes))?,
    })
}

const MOST_SEVERE: u8 = 24;

fn optional_id<T>(
    bytes: &[u8],
    read: impl Fn(&[u8]) -> Result<T, InvalidId>,
) -> Result<Option<T>, DecodeError> {
    if bytes.is_empty() {
        return Ok(None);
    }
    match read(bytes) {
        Ok(id) => Ok(Some(id)),
        Err(InvalidId::Zero) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn severity_of(number: i32, text: &str) -> Severity {
    match number {
        1..=4 => Severity::Trace,
        5..=8 => Severity::Debug,
        9..=12 => Severity::Info,
        13..=16 => Severity::Warn,
        17..=20 => Severity::Error,
        21..=24 => Severity::Fatal,
        _ => severity_named(text),
    }
}

fn severity_named(text: &str) -> Severity {
    let spelled = text.trim().to_ascii_lowercase();
    let starts = |prefix: &str| spelled.starts_with(prefix);
    if starts("trace") {
        Severity::Trace
    } else if starts("debug") {
        Severity::Debug
    } else if starts("warn") {
        Severity::Warn
    } else if starts("err") {
        Severity::Error
    } else if starts("fatal") || starts("crit") || starts("emerg") || starts("alert") {
        Severity::Fatal
    } else {
        Severity::Info
    }
}

fn body_of(body: Option<AnyValue>) -> Box<str> {
    match body.and_then(|body| body.value) {
        None => Box::default(),
        Some(Value::StringValue(text)) => text.into_boxed_str(),
        Some(other) => json_of(other).to_string().into_boxed_str(),
    }
}

fn json_of(value: Value) -> Json {
    match value {
        Value::StringValue(text) => Json::String(text),
        Value::BoolValue(flag) => Json::Bool(flag),
        Value::IntValue(number) => Json::Number(number.into()),
        Value::DoubleValue(number) => Number::from_f64(number).map_or(Json::Null, Json::Number),
        Value::BytesValue(bytes) => Json::String(hex::encode(bytes)),
        Value::ArrayValue(array) => Json::Array(array.values.into_iter().map(any_json).collect()),
        Value::KvlistValue(list) => Json::Object(
            list.values
                .into_iter()
                .map(|pair| (pair.key, pair.value.map_or(Json::Null, any_json)))
                .collect::<Map<String, Json>>(),
        ),
        Value::StringValueStrindex(_) => Json::Null,
    }
}

fn any_json(value: AnyValue) -> Json {
    value.value.map_or(Json::Null, json_of)
}
