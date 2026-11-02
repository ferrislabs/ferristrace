use ferristrace_domain::Severity;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::common::v1::{
    AnyValue, ArrayValue, KeyValue, KeyValueList, any_value::Value,
};
use opentelemetry_proto::tonic::logs::v1::{LogRecord as ProtoRecord, ResourceLogs, ScopeLogs};
use opentelemetry_proto::tonic::resource::v1::Resource;
use prost::Message;

use crate::logs::{decode_logs, logs_of, severity_of};
use crate::{DecodeError, UNKNOWN_SERVICE};

fn value(value: Value) -> AnyValue {
    AnyValue { value: Some(value) }
}

fn pair(key: &str, held: Value) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(value(held)),
        ..KeyValue::default()
    }
}

fn text(key: &str, held: &str) -> KeyValue {
    pair(key, Value::StringValue(held.to_owned()))
}

fn a_record() -> ProtoRecord {
    ProtoRecord {
        time_unix_nano: 100,
        observed_time_unix_nano: 150,
        severity_number: 17,
        severity_text: "ERROR".to_owned(),
        body: Some(value(Value::StringValue("payment declined".to_owned()))),
        attributes: vec![text("order.id", "o-42")],
        trace_id: vec![9u8; 16],
        span_id: vec![4u8; 8],
        ..ProtoRecord::default()
    }
}

fn a_request(record: ProtoRecord, resource: Option<Resource>) -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource,
            scope_logs: vec![ScopeLogs {
                log_records: vec![record],
                ..ScopeLogs::default()
            }],
            ..ResourceLogs::default()
        }],
    }
}

fn a_resource(attributes: Vec<KeyValue>) -> Resource {
    Resource {
        attributes,
        ..Resource::default()
    }
}

fn one(record: ProtoRecord) -> ferristrace_domain::LogRecord {
    let mut records =
        decode_logs(&a_request(record, None).encode_to_vec()).expect("a valid request");
    assert_eq!(records.len(), 1);
    records.remove(0)
}

#[test]
fn a_request_carries_its_records_service_and_resource_into_the_domain() {
    let request = a_request(
        a_record(),
        Some(a_resource(vec![
            text("service.name", "checkout"),
            text("deployment.environment", "prod"),
        ])),
    );

    let records = decode_logs(&request.encode_to_vec()).expect("a valid request");

    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(&*record.service_name, "checkout");
    assert_eq!(record.time_unix_nanos, 100);
    assert_eq!(record.observed_unix_nanos, 150);
    assert_eq!(record.severity, Severity::Error);
    assert_eq!(record.severity_number, 17);
    assert_eq!(&*record.body, "payment declined");
    assert_eq!(
        record.attributes.get("order.id").map(|v| &**v),
        Some("o-42")
    );
    assert_eq!(
        record.resource.get("deployment.environment").map(|v| &**v),
        Some("prod")
    );
    assert_eq!(
        record.resource.get("service.name").map(|v| &**v),
        Some("checkout")
    );
    assert_eq!(record.trace_id.map(|id| *id.as_bytes()), Some([9u8; 16]));
    assert_eq!(record.span_id.map(|id| *id.as_bytes()), Some([4u8; 8]));
}

#[test]
fn a_resource_without_a_service_name_falls_back() {
    assert_eq!(&*one(a_record()).service_name, UNKNOWN_SERVICE);
}

#[test]
fn every_severity_number_range_lands_on_its_level() {
    let expected = [
        (1..=4, Severity::Trace),
        (5..=8, Severity::Debug),
        (9..=12, Severity::Info),
        (13..=16, Severity::Warn),
        (17..=20, Severity::Error),
        (21..=24, Severity::Fatal),
    ];
    for (numbers, severity) in expected {
        for number in numbers {
            assert_eq!(severity_of(number, ""), severity, "number {number}");
        }
    }
}

#[test]
fn without_a_number_the_text_decides_the_level() {
    assert_eq!(severity_of(0, "WARNING"), Severity::Warn);
    assert_eq!(severity_of(0, "warn"), Severity::Warn);
    assert_eq!(severity_of(0, "Error"), Severity::Error);
    assert_eq!(severity_of(0, "err"), Severity::Error);
    assert_eq!(severity_of(0, "DEBUG"), Severity::Debug);
    assert_eq!(severity_of(0, "trace"), Severity::Trace);
    assert_eq!(severity_of(0, "FATAL"), Severity::Fatal);
    assert_eq!(severity_of(0, "critical"), Severity::Fatal);
    assert_eq!(severity_of(0, "information"), Severity::Info);
    assert_eq!(severity_of(99, "error"), Severity::Error);
}

#[test]
fn neither_number_nor_text_reads_as_info() {
    assert_eq!(severity_of(0, ""), Severity::Info);
    assert_eq!(severity_of(0, "chatty"), Severity::Info);
}

#[test]
fn a_record_keeps_the_severity_number_it_was_sent_with() {
    let mut record = a_record();
    record.severity_number = 0;
    record.severity_text = "warning".to_owned();

    let decoded = one(record);

    assert_eq!(decoded.severity, Severity::Warn);
    assert_eq!(decoded.severity_number, 0);
}

#[test]
fn a_body_that_is_not_text_is_kept_as_json() {
    let mut record = a_record();
    record.body = Some(value(Value::KvlistValue(KeyValueList {
        values: vec![
            text("user", "ada"),
            pair("attempt", Value::IntValue(3)),
            pair(
                "tags",
                Value::ArrayValue(ArrayValue {
                    values: vec![
                        value(Value::BoolValue(true)),
                        value(Value::DoubleValue(1.5)),
                        value(Value::StringValue("a \"quoted\" word".to_owned())),
                    ],
                }),
            ),
        ],
    })));

    let body = one(record).body;

    let parsed: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        parsed,
        serde_json::json!({"user": "ada", "attempt": 3, "tags": [true, 1.5, "a \"quoted\" word"]})
    );
}

#[test]
fn a_scalar_body_is_rendered_as_its_text() {
    let mut number = a_record();
    number.body = Some(value(Value::IntValue(42)));
    let mut bytes = a_record();
    bytes.body = Some(value(Value::BytesValue(vec![0xab, 0x01])));
    let mut missing = a_record();
    missing.body = None;

    assert_eq!(&*one(number).body, "42");
    assert_eq!(&*one(bytes).body, "\"ab01\"");
    assert_eq!(&*one(missing).body, "");
}

#[test]
fn a_record_without_a_time_takes_its_observed_time() {
    let mut record = a_record();
    record.time_unix_nano = 0;

    let decoded = one(record);

    assert_eq!(decoded.time_unix_nanos, 150);
    assert_eq!(decoded.observed_unix_nanos, 150);
}

#[test]
fn a_record_without_an_observed_time_takes_its_time() {
    let mut record = a_record();
    record.observed_time_unix_nano = 0;

    assert_eq!(one(record).observed_unix_nanos, 100);
}

#[test]
fn absent_or_zero_ids_mean_the_record_belongs_to_no_trace() {
    let mut absent = a_record();
    absent.trace_id = Vec::new();
    absent.span_id = Vec::new();
    let mut zero = a_record();
    zero.trace_id = vec![0u8; 16];
    zero.span_id = vec![0u8; 8];

    let absent = one(absent);
    let zero = one(zero);

    assert_eq!((absent.trace_id, absent.span_id), (None, None));
    assert_eq!((zero.trace_id, zero.span_id), (None, None));
}

#[test]
fn an_id_of_the_wrong_length_is_refused() {
    let mut record = a_record();
    record.trace_id = vec![1u8; 5];

    let outcome = decode_logs(&a_request(record, None).encode_to_vec());

    assert!(matches!(outcome, Err(DecodeError::Id(_))));
}

#[test]
fn a_truncated_payload_is_refused() {
    assert!(matches!(
        decode_logs(&[0xff, 0xff, 0xff]),
        Err(DecodeError::Protobuf(_))
    ));
}

#[test]
fn every_record_of_every_resource_arrives_with_its_own_service() {
    let request = ExportLogsServiceRequest {
        resource_logs: vec![
            ResourceLogs {
                resource: Some(a_resource(vec![text("service.name", "checkout")])),
                scope_logs: vec![ScopeLogs {
                    log_records: vec![a_record(), a_record()],
                    ..ScopeLogs::default()
                }],
                ..ResourceLogs::default()
            },
            ResourceLogs {
                resource: Some(a_resource(vec![text("service.name", "payments")])),
                scope_logs: vec![ScopeLogs {
                    log_records: vec![a_record()],
                    ..ScopeLogs::default()
                }],
                ..ResourceLogs::default()
            },
        ],
    };

    let records = logs_of(request).expect("a valid request");

    assert_eq!(
        records.iter().map(|r| &*r.service_name).collect::<Vec<_>>(),
        vec!["checkout", "checkout", "payments"]
    );
}
