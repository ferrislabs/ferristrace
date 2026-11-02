use ferristrace_domain::{InvalidId, Span, SpanEvent, SpanId, SpanKind, SpanStatus, TraceId};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::trace::v1::span::Event as ProtoEvent;
use opentelemetry_proto::tonic::trace::v1::span::SpanKind as ProtoKind;
use opentelemetry_proto::tonic::trace::v1::status::StatusCode;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, Span as ProtoSpan};
use prost::Message;

use crate::attributes;

pub const UNKNOWN_SERVICE: &str = "unknown_service";
const SERVICE_NAME: &str = "service.name";

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("the payload is not a valid OTLP export request")]
    Protobuf(#[from] prost::DecodeError),
    #[error(transparent)]
    Id(#[from] InvalidId),
}

pub fn decode_traces(payload: &[u8]) -> Result<Vec<Span>, DecodeError> {
    spans_of(ExportTraceServiceRequest::decode(payload)?)
}

pub fn spans_of(request: ExportTraceServiceRequest) -> Result<Vec<Span>, DecodeError> {
    let mut spans = Vec::with_capacity(span_count(&request.resource_spans));
    for resource_spans in request.resource_spans {
        let service = service_name(&resource_spans);
        for scope_spans in resource_spans.scope_spans {
            for span in scope_spans.spans {
                spans.push(convert(span, &service)?);
            }
        }
    }
    Ok(spans)
}

fn span_count(resource_spans: &[ResourceSpans]) -> usize {
    resource_spans
        .iter()
        .flat_map(|resource| resource.scope_spans.iter())
        .map(|scope| scope.spans.len())
        .sum()
}

fn service_name(resource_spans: &ResourceSpans) -> Box<str> {
    resource_spans
        .resource
        .as_ref()
        .and_then(|resource| attributes::lookup(&resource.attributes, SERVICE_NAME))
        .unwrap_or_else(|| UNKNOWN_SERVICE.into())
}

fn convert(span: ProtoSpan, service: &str) -> Result<Span, DecodeError> {
    let ProtoSpan {
        trace_id,
        span_id,
        parent_span_id,
        name,
        kind,
        start_time_unix_nano,
        end_time_unix_nano,
        attributes: pairs,
        status,
        events,
        ..
    } = span;

    let parent = if parent_span_id.is_empty() {
        None
    } else {
        Some(SpanId::try_from(parent_span_id.as_slice())?)
    };

    Ok(Span {
        trace_id: TraceId::try_from(trace_id.as_slice())?,
        span_id: SpanId::try_from(span_id.as_slice())?,
        parent,
        name: name.into_boxed_str(),
        service_name: service.into(),
        kind: kind_of(kind),
        status: status_of(status.map(|status| status.code)),
        start_unix_nanos: start_time_unix_nano,
        end_unix_nanos: end_time_unix_nano,
        attributes: attributes::collect(pairs),
        events: events_of(events),
    })
}

const MOST_EVENTS: usize = 128;

fn events_of(events: Vec<ProtoEvent>) -> Vec<SpanEvent> {
    events
        .into_iter()
        .take(MOST_EVENTS)
        .map(|event| SpanEvent {
            time_unix_nanos: event.time_unix_nano,
            name: event.name.into_boxed_str(),
            attributes: attributes::collect(event.attributes),
        })
        .collect()
}

fn kind_of(code: i32) -> SpanKind {
    match ProtoKind::try_from(code) {
        Ok(ProtoKind::Internal) => SpanKind::Internal,
        Ok(ProtoKind::Server) => SpanKind::Server,
        Ok(ProtoKind::Client) => SpanKind::Client,
        Ok(ProtoKind::Producer) => SpanKind::Producer,
        Ok(ProtoKind::Consumer) => SpanKind::Consumer,
        Ok(ProtoKind::Unspecified) | Err(_) => SpanKind::Unspecified,
    }
}

fn status_of(code: Option<i32>) -> SpanStatus {
    match code.map(StatusCode::try_from) {
        Some(Ok(StatusCode::Ok)) => SpanStatus::Ok,
        Some(Ok(StatusCode::Error)) => SpanStatus::Error,
        Some(Ok(StatusCode::Unset) | Err(_)) | None => SpanStatus::Unset,
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{SpanKind, SpanStatus};
    use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
    use opentelemetry_proto::tonic::resource::v1::Resource;
    use opentelemetry_proto::tonic::trace::v1::{ScopeSpans, Status};
    use prost::Message;

    use super::{
        DecodeError, ExportTraceServiceRequest, MOST_EVENTS, ProtoEvent, ProtoSpan, ResourceSpans,
        UNKNOWN_SERVICE, decode_traces,
    };

    fn pair(key: &str, value: Value) -> KeyValue {
        KeyValue {
            key: key.to_owned(),
            value: Some(AnyValue { value: Some(value) }),
            ..KeyValue::default()
        }
    }

    fn text(key: &str, value: &str) -> KeyValue {
        pair(key, Value::StringValue(value.to_owned()))
    }

    fn a_proto_span() -> ProtoSpan {
        ProtoSpan {
            trace_id: vec![9u8; 16],
            span_id: vec![4u8; 8],
            parent_span_id: Vec::new(),
            name: "GET /orders".to_owned(),
            kind: 2,
            start_time_unix_nano: 10,
            end_time_unix_nano: 20,
            status: Some(Status {
                code: 1,
                ..Status::default()
            }),
            ..ProtoSpan::default()
        }
    }

    fn a_request(span: ProtoSpan, resource: Option<Resource>) -> ExportTraceServiceRequest {
        ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource,
                scope_spans: vec![ScopeSpans {
                    spans: vec![span],
                    ..ScopeSpans::default()
                }],
                ..ResourceSpans::default()
            }],
        }
    }

    fn a_resource(attributes: Vec<KeyValue>) -> Resource {
        Resource {
            attributes,
            ..Resource::default()
        }
    }

    #[test]
    fn a_request_carries_its_spans_and_service_into_the_domain() {
        let request = a_request(
            a_proto_span(),
            Some(a_resource(vec![text("service.name", "orders")])),
        );

        let spans = decode_traces(&request.encode_to_vec()).expect("a valid request");

        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        assert_eq!(&*span.service_name, "orders");
        assert_eq!(&*span.name, "GET /orders");
        assert_eq!(span.kind, SpanKind::Server);
        assert_eq!(span.status, SpanStatus::Ok);
        assert_eq!(span.start_unix_nanos, 10);
        assert_eq!(span.end_unix_nanos, 20);
        assert_eq!(span.parent, None);
    }

    #[test]
    fn a_resource_without_a_service_name_falls_back() {
        let request = a_request(a_proto_span(), None);

        let spans = decode_traces(&request.encode_to_vec()).expect("a valid request");

        assert_eq!(&*spans[0].service_name, UNKNOWN_SERVICE);
    }

    #[test]
    fn an_all_zero_trace_id_is_refused() {
        let mut span = a_proto_span();
        span.trace_id = vec![0u8; 16];
        let request = a_request(span, None);

        let outcome = decode_traces(&request.encode_to_vec());

        assert!(matches!(outcome, Err(DecodeError::Id(_))));
    }

    #[test]
    fn a_truncated_payload_is_refused() {
        let outcome = decode_traces(&[0xff, 0xff, 0xff]);

        assert!(matches!(outcome, Err(DecodeError::Protobuf(_))));
    }

    #[test]
    fn typed_attributes_become_text_and_unsupported_ones_are_dropped() {
        let mut span = a_proto_span();
        span.attributes = vec![
            text("http.route", "/orders"),
            pair("http.status_code", Value::IntValue(200)),
            pair("retried", Value::BoolValue(true)),
            pair("payload", Value::BytesValue(vec![1, 2])),
        ];
        let request = a_request(span, None);

        let spans = decode_traces(&request.encode_to_vec()).expect("a valid request");

        let attributes = &spans[0].attributes;
        assert_eq!(attributes.get("http.route").map(|v| &**v), Some("/orders"));
        assert_eq!(
            attributes.get("http.status_code").map(|v| &**v),
            Some("200")
        );
        assert_eq!(attributes.get("retried").map(|v| &**v), Some("true"));
        assert_eq!(attributes.get("payload"), None);
    }

    #[test]
    fn a_parent_span_id_is_kept() {
        let mut span = a_proto_span();
        span.parent_span_id = vec![5u8; 8];
        let request = a_request(span, None);

        let spans = decode_traces(&request.encode_to_vec()).expect("a valid request");

        assert_eq!(spans[0].parent.map(|id| *id.as_bytes()), Some([5u8; 8]));
    }

    fn an_event(name: &str, at: u64, attributes: Vec<KeyValue>) -> ProtoEvent {
        ProtoEvent {
            time_unix_nano: at,
            name: name.to_owned(),
            attributes,
            ..ProtoEvent::default()
        }
    }

    #[test]
    fn an_exception_arrives_as_an_event_with_its_attributes() {
        let mut span = a_proto_span();
        span.events = vec![an_event(
            "exception",
            15,
            vec![
                text("exception.type", "PspTimeoutError"),
                text("exception.message", "deadline exceeded after 960ms"),
                text("exception.stacktrace", "at charge (psp/client.ts:88)"),
            ],
        )];
        let request = a_request(span, None);

        let spans = decode_traces(&request.encode_to_vec()).expect("a valid request");

        let events = &spans[0].events;
        assert_eq!(events.len(), 1);
        assert_eq!(&*events[0].name, "exception");
        assert_eq!(events[0].time_unix_nanos, 15);
        assert_eq!(
            events[0].attributes.get("exception.type").map(|v| &**v),
            Some("PspTimeoutError")
        );
        assert_eq!(
            events[0]
                .attributes
                .get("exception.stacktrace")
                .map(|v| &**v),
            Some("at charge (psp/client.ts:88)")
        );
    }

    #[test]
    fn events_keep_the_order_the_span_recorded_them_in() {
        let mut span = a_proto_span();
        span.events = vec![
            an_event("retrying", 11, Vec::new()),
            an_event("exception", 19, Vec::new()),
        ];
        let request = a_request(span, None);

        let spans = decode_traces(&request.encode_to_vec()).expect("a valid request");

        assert_eq!(
            spans[0]
                .events
                .iter()
                .map(|event| &*event.name)
                .collect::<Vec<_>>(),
            vec!["retrying", "exception"]
        );
    }

    #[test]
    fn a_span_with_no_events_carries_none() {
        let request = a_request(a_proto_span(), None);

        let spans = decode_traces(&request.encode_to_vec()).expect("a valid request");

        assert!(spans[0].events.is_empty());
    }

    #[test]
    fn a_span_carrying_more_events_than_the_limit_keeps_the_first_of_them() {
        let mut span = a_proto_span();
        span.events = (0..MOST_EVENTS + 40)
            .map(|at| an_event(&format!("step-{at}"), at as u64, Vec::new()))
            .collect();
        let request = a_request(span, None);

        let spans = decode_traces(&request.encode_to_vec()).expect("a valid request");

        assert_eq!(spans[0].events.len(), MOST_EVENTS);
        assert_eq!(&*spans[0].events[0].name, "step-0");
        assert_eq!(
            &*spans[0].events[MOST_EVENTS - 1].name,
            &*format!("step-{}", MOST_EVENTS - 1)
        );
    }

    #[test]
    fn an_event_attribute_that_does_not_render_is_dropped_and_the_event_stays() {
        let mut span = a_proto_span();
        span.events = vec![an_event(
            "exception",
            15,
            vec![
                text("exception.type", "PspTimeoutError"),
                pair("exception.stacktrace", Value::BytesValue(vec![1, 2])),
            ],
        )];
        let request = a_request(span, None);

        let spans = decode_traces(&request.encode_to_vec()).expect("a valid request");

        assert_eq!(spans[0].events.len(), 1);
        assert_eq!(spans[0].events[0].attributes.len(), 1);
        assert_eq!(
            spans[0].events[0].attributes.get("exception.stacktrace"),
            None
        );
    }

    mod proptests {
        use ferristrace_domain::{InvalidId, SpanId, TraceId};
        use proptest::prelude::*;

        use super::*;

        fn nonzero_id(width: usize) -> impl Strategy<Value = Vec<u8>> {
            (1u8..=u8::MAX, prop::collection::vec(any::<u8>(), width - 1)).prop_map(
                |(first, rest)| {
                    let mut bytes = vec![first];
                    bytes.extend(rest);
                    bytes
                },
            )
        }

        fn id_of_the_wrong_length(correct: usize) -> impl Strategy<Value = Vec<u8>> {
            (0usize..32)
                .prop_filter("the correct width is not a wrong one", move |length| {
                    *length != correct
                })
                .prop_flat_map(|length| prop::collection::vec(any::<u8>(), length))
        }

        fn short_text() -> impl Strategy<Value = String> {
            "[a-zA-Z0-9 /_.-]{0,12}"
        }

        fn short_key() -> impl Strategy<Value = String> {
            "[a-z][a-z0-9._]{0,10}"
        }

        fn near_a_numeric_ceiling() -> impl Strategy<Value = u64> {
            prop_oneof![
                (u64::MAX - 10..=u64::MAX),
                ((i64::MAX as u64 - 10)..=(i64::MAX as u64)),
            ]
        }

        fn a_well_formed_span() -> impl Strategy<Value = ProtoSpan> {
            (
                nonzero_id(TraceId::WIDTH),
                nonzero_id(SpanId::WIDTH),
                short_text(),
                0i32..6,
                any::<u64>(),
                any::<u64>(),
            )
                .prop_map(|(trace_id, span_id, name, kind, start, end)| ProtoSpan {
                    trace_id,
                    span_id,
                    name,
                    kind,
                    start_time_unix_nano: start,
                    end_time_unix_nano: end,
                    ..ProtoSpan::default()
                })
        }

        fn a_request_of_many(spans: Vec<ProtoSpan>) -> ExportTraceServiceRequest {
            ExportTraceServiceRequest {
                resource_spans: vec![ResourceSpans {
                    resource: None,
                    scope_spans: vec![ScopeSpans {
                        spans,
                        ..ScopeSpans::default()
                    }],
                    ..ResourceSpans::default()
                }],
            }
        }

        proptest! {
            #[test]
            fn every_well_formed_span_the_request_carries_arrives_exactly_once(
                spans in prop::collection::vec(a_well_formed_span(), 1..5)
            ) {
                let request = a_request_of_many(spans.clone());

                let decoded = decode_traces(&request.encode_to_vec()).expect("every span is well formed");

                prop_assert_eq!(decoded.len(), spans.len());
                for (found, sent) in decoded.iter().zip(spans.iter()) {
                    prop_assert_eq!(found.span_id.as_bytes().as_slice(), sent.span_id.as_slice());
                    prop_assert_eq!(found.trace_id.as_bytes().as_slice(), sent.trace_id.as_slice());
                    prop_assert_eq!(&*found.name, sent.name.as_str());
                    prop_assert_eq!(found.start_unix_nanos, sent.start_time_unix_nano);
                    prop_assert_eq!(found.end_unix_nanos, sent.end_time_unix_nano);
                }
            }

            #[test]
            fn a_span_id_of_the_wrong_byte_length_is_refused_rather_than_coerced(
                bad_span_id in id_of_the_wrong_length(SpanId::WIDTH)
            ) {
                let mut span = a_proto_span();
                span.span_id = bad_span_id.clone();
                let request = a_request(span, None);

                let outcome = decode_traces(&request.encode_to_vec());

                match &outcome {
                    Err(DecodeError::Id(InvalidId::Length { expected, actual })) => {
                        prop_assert_eq!(*expected, SpanId::WIDTH);
                        prop_assert_eq!(*actual, bad_span_id.len());
                    }
                    other => prop_assert!(false, "{other:?}"),
                }
            }

            #[test]
            fn a_trace_id_of_the_wrong_byte_length_is_refused_rather_than_coerced(
                bad_trace_id in id_of_the_wrong_length(TraceId::WIDTH)
            ) {
                let mut span = a_proto_span();
                span.trace_id = bad_trace_id.clone();
                let request = a_request(span, None);

                let outcome = decode_traces(&request.encode_to_vec());

                match &outcome {
                    Err(DecodeError::Id(InvalidId::Length { expected, actual })) => {
                        prop_assert_eq!(*expected, TraceId::WIDTH);
                        prop_assert_eq!(*actual, bad_trace_id.len());
                    }
                    other => prop_assert!(false, "{other:?}"),
                }
            }

            #[test]
            fn an_attribute_key_survives_the_conversion(
                key in short_key(),
                value in short_text(),
            ) {
                let mut span = a_proto_span();
                span.attributes = vec![text(&key, &value)];
                let request = a_request(span, None);

                let decoded = decode_traces(&request.encode_to_vec()).expect("a well formed span");

                prop_assert_eq!(
                    decoded[0].attributes.get(key.as_str()).map(|v| &**v),
                    Some(value.as_str())
                );
            }

            #[test]
            fn a_repeated_attribute_key_resolves_the_same_way_every_time(
                key in short_key(),
                first in short_text(),
                second in short_text(),
            ) {
                let mut span = a_proto_span();
                span.attributes = vec![text(&key, &first), text(&key, &second)];
                let request = a_request(span, None);

                let once = decode_traces(&request.encode_to_vec()).expect("a well formed span");
                let twice = decode_traces(&request.encode_to_vec()).expect("a well formed span");

                prop_assert_eq!(
                    once[0].attributes.get(key.as_str()),
                    twice[0].attributes.get(key.as_str())
                );
                prop_assert_eq!(
                    once[0].attributes.get(key.as_str()).map(|v| &**v),
                    Some(second.as_str())
                );
                prop_assert_eq!(once[0].attributes.len(), 1);
            }

            #[test]
            fn a_timestamp_near_a_numeric_ceiling_does_not_panic_or_wrap(
                start in near_a_numeric_ceiling(),
                end in near_a_numeric_ceiling(),
            ) {
                let mut span = a_proto_span();
                span.start_time_unix_nano = start;
                span.end_time_unix_nano = end;
                let request = a_request(span, None);

                let decoded = decode_traces(&request.encode_to_vec()).expect("a well formed span");

                prop_assert_eq!(decoded[0].start_unix_nanos, start);
                prop_assert_eq!(decoded[0].end_unix_nanos, end);
            }
        }
    }
}
