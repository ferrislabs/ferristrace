use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use bytes::Bytes;
use ferristrace_domain::{
    DecompressedBytes, Ingest, Payload, QuotaPolicy, SpanSink, TenantDirectory, UsageMeter,
};
use ferristrace_otlp::decode_traces;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceResponse;
use prost::Message;
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;

use crate::auth::Bearer;
use crate::error::ApiError;

pub const MAX_DECOMPRESSED_BYTES: usize = 4 * 1024 * 1024;
const PROTOBUF: &str = "application/x-protobuf";

pub fn router<D, Q, S, M>(ingest: Arc<Ingest<D, Q, S, M>>) -> Router
where
    D: TenantDirectory + Send + Sync + 'static,
    Q: QuotaPolicy + Send + Sync + 'static,
    S: SpanSink + Send + Sync + 'static,
    M: UsageMeter + Send + Sync + 'static,
{
    Router::new()
        .route("/v1/traces", post(traces::<D, Q, S, M>))
        .layer(RequestBodyLimitLayer::new(MAX_DECOMPRESSED_BYTES))
        .layer(RequestDecompressionLayer::new().gzip(true))
        .with_state(ingest)
}

async fn traces<D, Q, S, M>(
    State(ingest): State<Arc<Ingest<D, Q, S, M>>>,
    headers: HeaderMap,
    Bearer(key): Bearer,
    body: Bytes,
) -> Result<Response, ApiError>
where
    D: TenantDirectory + Send + Sync + 'static,
    Q: QuotaPolicy + Send + Sync + 'static,
    S: SpanSink + Send + Sync + 'static,
    M: UsageMeter + Send + Sync + 'static,
{
    require_protobuf(&headers)?;
    let decompressed = DecompressedBytes::new(body.len() as u64);
    let spans = decode_traces(&body)?;
    ingest
        .accept(
            &key,
            Payload {
                spans,
                decompressed,
            },
        )
        .await?;
    Ok(accepted())
}

fn require_protobuf(headers: &HeaderMap) -> Result<(), ApiError> {
    let declared = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|base| base.trim().eq_ignore_ascii_case(PROTOBUF));
    if declared {
        Ok(())
    } else {
        Err(ApiError::UnsupportedContentType)
    }
}

fn accepted() -> Response {
    (
        StatusCode::OK,
        [(CONTENT_TYPE, HeaderValue::from_static(PROTOBUF))],
        ExportTraceServiceResponse::default().encode_to_vec(),
    )
        .into_response()
}
