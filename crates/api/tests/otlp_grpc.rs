use std::sync::Arc;

use ferristrace_api::trace_server;
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Ingestion, Stack};
use ferristrace_domain::{DecompressedBytes, NoPhotos, QuotaPolicy, TenantId, Unmetered};
use ferristrace_infra::{Allowance, MemoryMeter, MemorySink, StaticDirectory, StaticQuota};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_client::TraceServiceClient;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as ProtoSpan};
use prost::Message;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::metadata::MetadataValue;
use tonic::transport::Server;
use tonic::{Code, Request};
use uuid::Uuid;

const KEY: &str = "live-key";
const TENANT: u128 = 23;

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
    type Meter = Arc<MemoryMeter>;
    type Oidc = Inert;
    type Sink = Arc<MemorySink>;
    type Quota = Q;
    type Directory = StaticDirectory;
}

struct Running {
    endpoint: String,
    sink: Arc<MemorySink>,
    meter: Arc<MemoryMeter>,
    tenant: TenantId,
}

async fn serving() -> Running {
    serving_with(Unmetered).await
}

async fn serving_with<Q: QuotaPolicy + Send + Sync + 'static>(quota: Q) -> Running {
    let tenant = TenantId::new(Uuid::from_u128(TENANT));
    let sink = Arc::new(MemorySink::default());
    let meter = Arc::new(MemoryMeter::default());
    let ingest = Arc::new(Ingestion::<TestStack<Q>>::new(
        StaticDirectory::from_plaintext([(KEY, tenant)]),
        quota,
        Arc::clone(&sink),
        Arc::clone(&meter),
    ));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let endpoint = format!("http://{}", listener.local_addr().expect("a local address"));
    tokio::spawn(async move {
        Server::builder()
            .add_service(trace_server(ingest))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .expect("the gRPC server runs");
    });

    Running {
        endpoint,
        sink,
        meter,
        tenant,
    }
}

fn a_request(span_count: usize) -> ExportTraceServiceRequest {
    let spans = (0..span_count)
        .map(|index| ProtoSpan {
            trace_id: vec![2u8; 16],
            span_id: vec![u8::try_from(index % 250).unwrap_or(1) + 1; 8],
            name: format!("operation number {index}"),
            kind: 3,
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
                        value: Some(Value::StringValue("payments".to_owned())),
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
}

fn authorised(message: ExportTraceServiceRequest, key: &str) -> Request<ExportTraceServiceRequest> {
    let mut request = Request::new(message);
    let value: MetadataValue<_> = format!("Bearer {key}")
        .parse()
        .expect("a valid metadata value");
    request.metadata_mut().insert("authorization", value);
    request
}

#[tokio::test]
async fn a_grpc_export_with_a_known_key_is_accepted() {
    let running = serving().await;
    let mut client = TraceServiceClient::connect(running.endpoint.clone())
        .await
        .expect("the client connects");

    let response = client
        .export(authorised(a_request(2), KEY))
        .await
        .expect("the server accepts");

    assert!(response.into_inner().partial_success.is_none());
    assert_eq!(running.sink.accepted(), 2);
    assert_eq!(running.sink.recent()[0].0.tenant, running.tenant);
    assert_eq!(&*running.sink.recent()[0].1.service_name, "payments");
}

#[tokio::test]
async fn a_grpc_export_is_metered_at_its_decoded_size() {
    let running = serving().await;
    let mut client = TraceServiceClient::connect(running.endpoint.clone())
        .await
        .expect("the client connects");
    let message = a_request(16);
    let decoded_size = message.encoded_len() as u64;

    client
        .export(authorised(message, KEY))
        .await
        .expect("the server accepts");

    assert_eq!(running.meter.total_for(running.tenant), decoded_size);
}

#[tokio::test]
async fn a_grpc_export_with_an_unknown_key_is_refused() {
    let running = serving().await;
    let mut client = TraceServiceClient::connect(running.endpoint.clone())
        .await
        .expect("the client connects");

    let status = client
        .export(authorised(a_request(1), "wrong-key"))
        .await
        .expect_err("the server refuses");

    assert_eq!(status.code(), Code::Unauthenticated);
    assert_eq!(running.sink.accepted(), 0);
}

#[tokio::test]
async fn a_grpc_export_without_a_bearer_is_refused() {
    let running = serving().await;
    let mut client = TraceServiceClient::connect(running.endpoint.clone())
        .await
        .expect("the client connects");

    let status = client
        .export(Request::new(a_request(1)))
        .await
        .expect_err("the server refuses");

    assert_eq!(status.code(), Code::Unauthenticated);
    assert_eq!(running.sink.accepted(), 0);
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
async fn a_tenant_over_its_cap_is_told_the_resource_is_exhausted() {
    let running = serving_with(capped_at(0)).await;
    let mut client = TraceServiceClient::connect(running.endpoint.clone())
        .await
        .expect("the client connects");

    let status = client
        .export(authorised(a_request(1), KEY))
        .await
        .expect_err("the server refuses");

    assert_eq!(status.code(), Code::ResourceExhausted);
    assert_eq!(running.sink.accepted(), 0);
    assert_eq!(running.meter.total_for(running.tenant), 0);
}
