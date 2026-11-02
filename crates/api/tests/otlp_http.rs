use std::io::Write;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use ferristrace_api::router;
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Ingestion, Stack};
use ferristrace_domain::{Allowance, StaticQuota};
use ferristrace_domain::{DecompressedBytes, NoPhotos, QuotaPolicy, TenantId, Unmetered};
use ferristrace_infra::{MemoryMeter, MemorySink, StaticDirectory};
use flate2::Compression;
use flate2::write::GzEncoder;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as ProtoSpan};
use prost::Message;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "live-key";
const TENANT: u128 = 11;
const PROTOBUF: &str = "application/x-protobuf";
const TRACES: &str = "/v1/traces";

struct Fakes;

impl Untransacted for Fakes {}

struct TestStack<Q>(std::marker::PhantomData<Q>);

impl<Q: QuotaPolicy + Send + Sync + 'static> Stack for TestStack<Q> {
    type Repos = Fakes;
    type Clock = Inert;
    type Hasher = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = Inert;
    type LogStore = Inert;
    type Meter = Arc<MemoryMeter>;
    type Oidc = Inert;
    type Sink = Arc<MemorySink>;
    type Quota = Q;
    type Directory = StaticDirectory;
}

struct Harness {
    router: Router,
    sink: Arc<MemorySink>,
    meter: Arc<MemoryMeter>,
    tenant: TenantId,
}

fn harness() -> Harness {
    harness_with(Unmetered)
}

fn harness_with<Q: QuotaPolicy + Send + Sync + 'static>(quota: Q) -> Harness {
    let tenant = TenantId::new(Uuid::from_u128(TENANT));
    let sink = Arc::new(MemorySink::default());
    let meter = Arc::new(MemoryMeter::default());
    let ingest = Arc::new(Ingestion::<TestStack<Q>>::new(
        StaticDirectory::from_plaintext([(KEY, tenant)]),
        quota,
        Arc::clone(&sink),
        Arc::clone(&meter),
    ));
    Harness {
        router: router(ingest),
        sink,
        meter,
        tenant,
    }
}

fn a_payload(span_count: usize) -> Vec<u8> {
    let spans = (0..span_count)
        .map(|index| ProtoSpan {
            trace_id: vec![1u8; 16],
            span_id: vec![u8::try_from(index % 250).unwrap_or(1) + 1; 8],
            name: format!("operation number {index} in the checkout service"),
            kind: 2,
            start_time_unix_nano: 1,
            end_time_unix_nano: 2,
            ..ProtoSpan::default()
        })
        .collect();
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: Some(Resource {
                attributes: vec![KeyValue {
                    key: "service.name".to_owned(),
                    value: Some(AnyValue {
                        value: Some(Value::StringValue("checkout".to_owned())),
                    }),
                    ..KeyValue::default()
                }],
                ..Resource::default()
            }),
            scope_spans: vec![ScopeSpans {
                spans,
                ..ScopeSpans::default()
            }],
            ..ResourceSpans::default()
        }],
    }
    .encode_to_vec()
}

fn gzipped(plain: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(plain).expect("gzip write");
    encoder.finish().expect("gzip finish")
}

fn export(body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(TRACES)
        .header(header::CONTENT_TYPE, PROTOBUF)
        .header(header::AUTHORIZATION, format!("Bearer {KEY}"))
        .body(Body::from(body))
        .expect("a well formed request")
}

#[tokio::test]
async fn a_protobuf_export_with_a_known_key_is_accepted() {
    let harness = harness();

    let response = harness
        .router
        .clone()
        .oneshot(export(a_payload(1)))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some(PROTOBUF)
    );
    assert_eq!(harness.sink.accepted(), 1);
    assert_eq!(harness.sink.recent()[0].0.tenant, harness.tenant);
    assert_eq!(&*harness.sink.recent()[0].1.service_name, "checkout");
}

#[tokio::test]
async fn an_unknown_key_is_refused_and_nothing_is_stored() {
    let harness = harness();
    let request = Request::builder()
        .method("POST")
        .uri(TRACES)
        .header(header::CONTENT_TYPE, PROTOBUF)
        .header(header::AUTHORIZATION, "Bearer wrong-key")
        .body(Body::from(a_payload(1)))
        .expect("a well formed request");

    let response = harness
        .router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(harness.sink.accepted(), 0);
    assert_eq!(harness.meter.total_for(harness.tenant), 0);
}

#[tokio::test]
async fn a_request_without_a_bearer_is_refused() {
    let harness = harness();
    let request = Request::builder()
        .method("POST")
        .uri(TRACES)
        .header(header::CONTENT_TYPE, PROTOBUF)
        .body(Body::from(a_payload(1)))
        .expect("a well formed request");

    let response = harness
        .router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(harness.sink.accepted(), 0);
}

#[tokio::test]
async fn a_gzip_body_is_metered_at_its_decompressed_size() {
    let harness = harness();
    let plain = a_payload(64);
    let compressed = gzipped(&plain);
    assert!(
        compressed.len() < plain.len(),
        "the fixture must actually compress"
    );

    let request = Request::builder()
        .method("POST")
        .uri(TRACES)
        .header(header::CONTENT_TYPE, PROTOBUF)
        .header(header::CONTENT_ENCODING, "gzip")
        .header(header::AUTHORIZATION, format!("Bearer {KEY}"))
        .body(Body::from(compressed.clone()))
        .expect("a well formed request");

    let response = harness
        .router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(harness.sink.accepted(), 64);
    assert_eq!(
        harness.meter.total_for(harness.tenant),
        plain.len() as u64,
        "the meter must see the decompressed size, not the {} compressed bytes",
        compressed.len()
    );
}

#[tokio::test]
async fn a_wrong_content_type_is_refused() {
    let harness = harness();
    let request = Request::builder()
        .method("POST")
        .uri(TRACES)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {KEY}"))
        .body(Body::from(a_payload(1)))
        .expect("a well formed request");

    let response = harness
        .router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(harness.sink.accepted(), 0);
}

#[tokio::test]
async fn a_malformed_payload_is_refused() {
    let harness = harness();

    let response = harness
        .router
        .clone()
        .oneshot(export(vec![0xff, 0xff, 0xff]))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(harness.sink.accepted(), 0);
}

fn capped_at(bytes: u64) -> StaticQuota<MemoryMeter> {
    StaticQuota::new(
        MemoryMeter::default(),
        [(
            TenantId::new(Uuid::from_u128(TENANT)),
            Allowance::Capped(DecompressedBytes::new(bytes)),
        )],
    )
}

#[tokio::test]
async fn a_tenant_over_its_cap_gets_429_and_stores_nothing() {
    let harness = harness_with(capped_at(0));

    let response = harness
        .router
        .clone()
        .oneshot(export(a_payload(1)))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(harness.sink.accepted(), 0);
    assert_eq!(harness.meter.total_for(harness.tenant), 0);
}

#[tokio::test]
async fn a_tenant_inside_its_cap_is_served_as_before() {
    let harness = harness_with(capped_at(4 * 1024 * 1024));

    let response = harness
        .router
        .clone()
        .oneshot(export(a_payload(1)))
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(harness.sink.accepted(), 1);
}
