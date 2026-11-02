use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use bytes::Bytes;
use ferristrace_application::{Ingestion, Stack};
use ferristrace_domain::{DecompressedBytes, Payload};
use ferristrace_otlp::decode_traces;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceResponse;
use prost::Message;
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;

use crate::auth::Bearer;
use crate::error::ApiError;

pub const MAX_DECOMPRESSED_BYTES: usize = 4 * 1024 * 1024;
const PROTOBUF: &str = "application/x-protobuf";

pub fn router<S: Stack>(ingest: Arc<Ingestion<S>>) -> Router {
    Router::new()
        .route("/v1/traces", post(traces::<S>))
        .layer(RequestBodyLimitLayer::new(MAX_DECOMPRESSED_BYTES))
        .layer(RequestDecompressionLayer::new().gzip(true))
        .with_state(ingest)
}

async fn traces<S: Stack>(
    State(ingest): State<Arc<Ingestion<S>>>,
    headers: HeaderMap,
    Bearer(key): Bearer,
    body: Bytes,
) -> Result<Response, ApiError> {
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
