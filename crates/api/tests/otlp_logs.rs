use std::io::Write;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use ferristrace_api::{MAX_DECOMPRESSED_BYTES, logs_server, router, trace_server};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Ingestion, Stack};
use ferristrace_domain::{
    Allowance, DecompressedBytes, NoPhotos, QuotaPolicy, Severity, StaticQuota, TenantId, Unmetered,
};
use ferristrace_infra::{MemoryMeter, MemorySink, StaticDirectory};
use flate2::Compression;
use flate2::write::GzEncoder;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::logs::v1::logs_service_client::LogsServiceClient;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
use opentelemetry_proto::tonic::logs::v1::{LogRecord as ProtoRecord, ResourceLogs, ScopeLogs};
use opentelemetry_proto::tonic::resource::v1::Resource;
use prost::Message;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::metadata::MetadataValue;
use tonic::transport::Server;
use tonic::{Code, Request as GrpcRequest};
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "live-key";
const READ_KEY: &str = "ft_rd_not-an-ingest-key";
const TENANT: u128 = 31;
const PROTOBUF: &str = "application/x-protobuf";
const LOGS: &str = "/v1/logs";
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
    type MetricStore = Inert;
    type Meter = Arc<MemoryMeter>;
    type Oidc = Inert;
    type Sink = Arc<MemorySink>;
    type Quota = Q;
    type Directory = StaticDirectory;
    type Webhooks = Inert;
}

struct Harness<Q: QuotaPolicy + Send + Sync + 'static> {
    ingest: Arc<Ingestion<TestStack<Q>>>,
    sink: Arc<MemorySink>,
    meter: Arc<MemoryMeter>,
    tenant: TenantId,
}

impl<Q: QuotaPolicy + Send + Sync + 'static> Harness<Q> {
    fn router(&self) -> Router {
        router(Arc::clone(&self.ingest))
    }
}

fn harness() -> Harness<Unmetered> {
    harness_with(Unmetered)
}

fn harness_with<Q: QuotaPolicy + Send + Sync + 'static>(quota: Q) -> Harness<Q> {
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
        ingest,
        sink,
        meter,
        tenant,
    }
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

fn text(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(value.to_owned())),
        }),
        ..KeyValue::default()
    }
}

fn a_request(record_count: usize) -> ExportLogsServiceRequest {
    let log_records = (0..record_count)
        .map(|index| ProtoRecord {
            time_unix_nano: 1_700_000_000_000_000_000 + index as u64,
            severity_number: 17,
            body: Some(AnyValue {
                value: Some(Value::StringValue(format!(
                    "payment number {index} was declined by the issuer"
                ))),
            }),
            attributes: vec![text("order.id", "o-42")],
            trace_id: vec![5u8; 16],
            span_id: vec![6u8; 8],
            ..ProtoRecord::default()
        })
        .collect();
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![text("service.name", "payments")],
                ..Resource::default()
            }),
            scope_logs: vec![ScopeLogs {
                log_records,
                ..ScopeLogs::default()
            }],
            ..ResourceLogs::default()
        }],
    }
}

fn a_payload(record_count: usize) -> Vec<u8> {
    a_request(record_count).encode_to_vec()
}

fn gzipped(plain: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(plain).expect("gzip write");
    encoder.finish().expect("gzip finish")
}

fn posted(uri: &str, content_type: &str, key: Option<&str>, body: Vec<u8>) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, content_type);
    if let Some(key) = key {
        request = request.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    request
        .body(Body::from(body))
        .expect("a well formed request")
}

async fn status_of(router: Router, request: Request<Body>) -> StatusCode {
    router
        .oneshot(request)
        .await
        .expect("the router answers")
        .status()
}

#[tokio::test]
async fn a_protobuf_logs_export_with_an_ingest_key_is_accepted_and_metered() {
    let harness = harness();
    let payload = a_payload(3);

    let response = harness
        .router()
        .oneshot(posted(LOGS, PROTOBUF, Some(KEY), payload.clone()))
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
    assert_eq!(harness.sink.logs_accepted(), 3);
    assert_eq!(harness.sink.accepted(), 0, "no span was written");
    let (scope, record) = &harness.sink.recent_logs()[0];
    assert_eq!(scope.tenant, harness.tenant);
    assert_eq!(&*record.service_name, "payments");
    assert_eq!(record.severity, Severity::Error);
    assert_eq!(record.trace_id.map(|id| *id.as_bytes()), Some([5u8; 16]));
    assert_eq!(
        harness.meter.total_for(harness.tenant),
        payload.len() as u64
    );
}

#[tokio::test]
async fn a_gzip_logs_export_is_metered_at_its_decompressed_size() {
    let harness = harness();
    let plain = a_payload(64);
    let compressed = gzipped(&plain);
    assert!(compressed.len() < plain.len());
    let mut request = posted(LOGS, PROTOBUF, Some(KEY), compressed);
    request
        .headers_mut()
        .insert(header::CONTENT_ENCODING, "gzip".parse().expect("a header"));

    let status = status_of(harness.router(), request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(harness.sink.logs_accepted(), 64);
    assert_eq!(harness.meter.total_for(harness.tenant), plain.len() as u64);
}

#[tokio::test]
async fn a_logs_export_is_refused_without_an_ingest_key() {
    let harness = harness();

    let none = status_of(harness.router(), posted(LOGS, PROTOBUF, None, a_payload(1))).await;
    let read = status_of(
        harness.router(),
        posted(LOGS, PROTOBUF, Some(READ_KEY), a_payload(1)),
    )
    .await;

    assert_eq!(none, StatusCode::UNAUTHORIZED);
    assert_eq!(read, StatusCode::UNAUTHORIZED);
    assert_eq!(harness.sink.logs_accepted(), 0);
    assert_eq!(harness.meter.total_for(harness.tenant), 0);
}

#[tokio::test]
async fn a_json_logs_export_is_refused_like_a_json_trace_export() {
    let harness = harness();
    let json = br#"{"resourceLogs":[]}"#.to_vec();

    let logs = status_of(
        harness.router(),
        posted(LOGS, "application/json", Some(KEY), json.clone()),
    )
    .await;
    let traces = status_of(
        harness.router(),
        posted(TRACES, "application/json", Some(KEY), json),
    )
    .await;

    assert_eq!(logs, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(logs, traces);
    assert_eq!(harness.sink.logs_accepted(), 0);
}

#[tokio::test]
async fn a_malformed_logs_payload_is_refused() {
    let harness = harness();

    let status = status_of(
        harness.router(),
        posted(LOGS, PROTOBUF, Some(KEY), vec![0xff, 0xff, 0xff]),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(harness.sink.logs_accepted(), 0);
}

#[tokio::test]
async fn an_oversize_logs_export_is_refused_rather_than_truncated() {
    let harness = harness();
    let oversize = vec![0u8; MAX_DECOMPRESSED_BYTES + 1];
    let mut bomb = posted(LOGS, PROTOBUF, Some(KEY), gzipped(&oversize));
    bomb.headers_mut()
        .insert(header::CONTENT_ENCODING, "gzip".parse().expect("a header"));

    let plain = status_of(
        harness.router(),
        posted(LOGS, PROTOBUF, Some(KEY), oversize.clone()),
    )
    .await;
    let compressed = status_of(harness.router(), bomb).await;

    assert_eq!(plain, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(compressed, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(harness.sink.logs_accepted(), 0);
    assert_eq!(harness.meter.total_for(harness.tenant), 0);
}

#[tokio::test]
async fn a_tenant_over_its_cap_gets_429_for_logs_and_stores_nothing() {
    let harness = harness_with(capped_at(0));

    let status = status_of(
        harness.router(),
        posted(LOGS, PROTOBUF, Some(KEY), a_payload(1)),
    )
    .await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(harness.sink.logs_accepted(), 0);
    assert_eq!(harness.meter.total_for(harness.tenant), 0);
}

#[tokio::test]
async fn logs_and_spans_count_against_one_quota() {
    let span_payload = {
        use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
        use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span};
        ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                scope_spans: vec![ScopeSpans {
                    spans: vec![Span {
                        trace_id: vec![1u8; 16],
                        span_id: vec![1u8; 8],
                        name: "work".to_owned(),
                        ..Span::default()
                    }],
                    ..ScopeSpans::default()
                }],
                ..ResourceSpans::default()
            }],
        }
        .encode_to_vec()
    };
    let logs = a_payload(1);
    let cap = (span_payload.len() + logs.len()) as u64;
    let meter = Arc::new(MemoryMeter::default());
    let tenant = TenantId::new(Uuid::from_u128(TENANT));
    let sink = Arc::new(MemorySink::default());
    let ingest = Arc::new(Ingestion::<TestStack<StaticQuota<Arc<MemoryMeter>>>>::new(
        StaticDirectory::from_plaintext([(KEY, tenant)]),
        StaticQuota::new(
            Arc::clone(&meter),
            [(tenant, Allowance::Capped(DecompressedBytes::new(cap)))],
        ),
        Arc::clone(&sink),
        Arc::clone(&meter),
    ));

    let first = status_of(
        router(Arc::clone(&ingest)),
        posted(TRACES, PROTOBUF, Some(KEY), span_payload),
    )
    .await;
    let second = status_of(
        router(Arc::clone(&ingest)),
        posted(LOGS, PROTOBUF, Some(KEY), logs.clone()),
    )
    .await;
    let third = status_of(
        router(Arc::clone(&ingest)),
        posted(LOGS, PROTOBUF, Some(KEY), logs),
    )
    .await;

    assert_eq!((first, second), (StatusCode::OK, StatusCode::OK));
    assert_eq!(third, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(meter.total_for(tenant), cap);
}

async fn grpc_serving<Q: QuotaPolicy + Send + Sync + 'static>(harness: &Harness<Q>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let endpoint = format!("http://{}", listener.local_addr().expect("a local address"));
    let ingest = Arc::clone(&harness.ingest);
    tokio::spawn(async move {
        Server::builder()
            .add_service(trace_server(Arc::clone(&ingest)))
            .add_service(logs_server(ingest))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .expect("the gRPC server runs");
    });
    endpoint
}

fn authorised(
    message: ExportLogsServiceRequest,
    key: Option<&str>,
) -> GrpcRequest<ExportLogsServiceRequest> {
    let mut request = GrpcRequest::new(message);
    if let Some(key) = key {
        let value: MetadataValue<_> = format!("Bearer {key}")
            .parse()
            .expect("a valid metadata value");
        request.metadata_mut().insert("authorization", value);
    }
    request
}

#[tokio::test]
async fn a_grpc_logs_export_with_an_ingest_key_is_accepted_and_metered() {
    let harness = harness();
    let endpoint = grpc_serving(&harness).await;
    let mut client = LogsServiceClient::connect(endpoint)
        .await
        .expect("the client connects");
    let message = a_request(2);
    let size = message.encoded_len() as u64;

    client
        .export(authorised(message, Some(KEY)))
        .await
        .expect("the server accepts");

    assert_eq!(harness.sink.logs_accepted(), 2);
    assert_eq!(&*harness.sink.recent_logs()[0].1.service_name, "payments");
    assert_eq!(harness.meter.total_for(harness.tenant), size);
}

#[tokio::test]
async fn a_grpc_logs_export_without_an_ingest_key_is_unauthenticated() {
    let harness = harness();
    let endpoint = grpc_serving(&harness).await;
    let mut client = LogsServiceClient::connect(endpoint)
        .await
        .expect("the client connects");

    let none = client
        .export(authorised(a_request(1), None))
        .await
        .expect_err("refused");
    let read = client
        .export(authorised(a_request(1), Some(READ_KEY)))
        .await
        .expect_err("refused");

    assert_eq!(none.code(), Code::Unauthenticated);
    assert_eq!(read.code(), Code::Unauthenticated);
    assert_eq!(harness.sink.logs_accepted(), 0);
}

#[tokio::test]
async fn a_grpc_logs_export_over_the_cap_is_told_the_resource_is_exhausted() {
    let harness = harness_with(capped_at(0));
    let endpoint = grpc_serving(&harness).await;
    let mut client = LogsServiceClient::connect(endpoint)
        .await
        .expect("the client connects");

    let refused = client
        .export(authorised(a_request(1), Some(KEY)))
        .await
        .expect_err("refused");

    assert_eq!(refused.code(), Code::ResourceExhausted);
    assert_eq!(harness.sink.logs_accepted(), 0);
}
