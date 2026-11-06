use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use bytes::Bytes;
use ferristrace_application::{Ingestion, Stack};
use ferristrace_domain::{DecompressedBytes, LogPayload, MetricPayload, Payload};
use ferristrace_otlp::{
    decode_logs, decode_logs_json, decode_metrics, decode_metrics_json, decode_traces,
    decode_traces_json,
};
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceResponse;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceResponse;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceResponse;
use prost::Message;
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;

use crate::auth::Bearer;
use crate::error::ApiError;

pub const MAX_DECOMPRESSED_BYTES: usize = 4 * 1024 * 1024;
const PROTOBUF: &str = "application/x-protobuf";
const JSON: &str = "application/json";

#[derive(Clone, Copy)]
enum Encoding {
    Protobuf,
    Json,
}

impl Encoding {
    fn media(self) -> &'static str {
        match self {
            Self::Protobuf => PROTOBUF,
            Self::Json => JSON,
        }
    }
}

pub fn router<S: Stack>(ingest: Arc<Ingestion<S>>) -> Router {
    Router::new()
        .route("/v1/traces", post(traces::<S>))
        .route("/v1/logs", post(logs::<S>))
        .route("/v1/metrics", post(metrics::<S>))
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
    let encoding = encoding_of(&headers)?;
    let decompressed = DecompressedBytes::new(body.len() as u64);
    let spans = match encoding {
        Encoding::Protobuf => decode_traces(&body)?,
        Encoding::Json => decode_traces_json(&body)?,
    };
    ingest
        .accept(
            &key,
            Payload {
                spans,
                decompressed,
            },
        )
        .await?;
    Ok(accepted(
        encoding,
        ExportTraceServiceResponse::default().encode_to_vec(),
    ))
}

async fn logs<S: Stack>(
    State(ingest): State<Arc<Ingestion<S>>>,
    headers: HeaderMap,
    Bearer(key): Bearer,
    body: Bytes,
) -> Result<Response, ApiError> {
    let encoding = encoding_of(&headers)?;
    let decompressed = DecompressedBytes::new(body.len() as u64);
    let records = match encoding {
        Encoding::Protobuf => decode_logs(&body)?,
        Encoding::Json => decode_logs_json(&body)?,
    };
    ingest
        .accept_logs(
            &key,
            LogPayload {
                records,
                decompressed,
            },
        )
        .await?;
    Ok(accepted(
        encoding,
        ExportLogsServiceResponse::default().encode_to_vec(),
    ))
}

async fn metrics<S: Stack>(
    State(ingest): State<Arc<Ingestion<S>>>,
    headers: HeaderMap,
    Bearer(key): Bearer,
    body: Bytes,
) -> Result<Response, ApiError> {
    let encoding = encoding_of(&headers)?;
    let decompressed = DecompressedBytes::new(body.len() as u64);
    let metrics = match encoding {
        Encoding::Protobuf => decode_metrics(&body)?,
        Encoding::Json => decode_metrics_json(&body)?,
    };
    ingest
        .accept_metrics(
            &key,
            MetricPayload {
                metrics,
                decompressed,
            },
        )
        .await?;
    Ok(accepted(
        encoding,
        ExportMetricsServiceResponse::default().encode_to_vec(),
    ))
}

fn encoding_of(headers: &HeaderMap) -> Result<Encoding, ApiError> {
    let base = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    match base {
        Some(base) if base.eq_ignore_ascii_case(PROTOBUF) => Ok(Encoding::Protobuf),
        Some(base) if base.eq_ignore_ascii_case(JSON) => Ok(Encoding::Json),
        _ => Err(ApiError::UnsupportedContentType),
    }
}

fn accepted(encoding: Encoding, protobuf: Vec<u8>) -> Response {
    let body = match encoding {
        Encoding::Protobuf => protobuf,
        Encoding::Json => b"{}".to_vec(),
    };
    (
        StatusCode::OK,
        [(CONTENT_TYPE, HeaderValue::from_static(encoding.media()))],
        body,
    )
        .into_response()
}
