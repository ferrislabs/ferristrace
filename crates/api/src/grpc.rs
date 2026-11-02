use std::fmt;
use std::sync::Arc;

use ferristrace_application::{Ingestion, Stack};
use ferristrace_domain::{DecompressedBytes, LogPayload, MetricPayload, Payload};
use ferristrace_otlp::{logs_of, metrics_of, spans_of};
use opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::{
    LogsService, LogsServiceServer,
};
use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_server::{
    MetricsService, MetricsServiceServer,
};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use prost::Message;
use tonic::codec::CompressionEncoding;
use tonic::{Request, Response, Status};

use crate::auth;
use crate::error::ApiError;

pub struct TraceGateway<S: Stack> {
    ingest: Arc<Ingestion<S>>,
}

impl<S: Stack> TraceGateway<S> {
    pub fn new(ingest: Arc<Ingestion<S>>) -> Self {
        Self { ingest }
    }
}

pub fn trace_server<S: Stack>(ingest: Arc<Ingestion<S>>) -> TraceServiceServer<TraceGateway<S>> {
    TraceServiceServer::new(TraceGateway::new(ingest)).accept_compressed(CompressionEncoding::Gzip)
}

impl<S: Stack> fmt::Debug for TraceGateway<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TraceGateway")
            .finish_non_exhaustive()
    }
}

#[tonic::async_trait]
impl<S: Stack> TraceService for TraceGateway<S> {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        let key = auth::key_of(request.metadata())?;
        let message = request.into_inner();
        let decompressed = DecompressedBytes::new(message.encoded_len() as u64);
        let spans = spans_of(message).map_err(ApiError::from)?;
        self.ingest
            .accept(
                &key,
                Payload {
                    spans,
                    decompressed,
                },
            )
            .await
            .map_err(ApiError::from)?;
        Ok(Response::new(ExportTraceServiceResponse::default()))
    }
}

pub struct LogsGateway<S: Stack> {
    ingest: Arc<Ingestion<S>>,
}

impl<S: Stack> LogsGateway<S> {
    pub fn new(ingest: Arc<Ingestion<S>>) -> Self {
        Self { ingest }
    }
}

pub fn logs_server<S: Stack>(ingest: Arc<Ingestion<S>>) -> LogsServiceServer<LogsGateway<S>> {
    LogsServiceServer::new(LogsGateway::new(ingest)).accept_compressed(CompressionEncoding::Gzip)
}

impl<S: Stack> fmt::Debug for LogsGateway<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogsGateway")
            .finish_non_exhaustive()
    }
}

#[tonic::async_trait]
impl<S: Stack> LogsService for LogsGateway<S> {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        let key = auth::key_of(request.metadata())?;
        let message = request.into_inner();
        let decompressed = DecompressedBytes::new(message.encoded_len() as u64);
        let records = logs_of(message).map_err(ApiError::from)?;
        self.ingest
            .accept_logs(
                &key,
                LogPayload {
                    records,
                    decompressed,
                },
            )
            .await
            .map_err(ApiError::from)?;
        Ok(Response::new(ExportLogsServiceResponse::default()))
    }
}

pub struct MetricsGateway<S: Stack> {
    ingest: Arc<Ingestion<S>>,
}

impl<S: Stack> MetricsGateway<S> {
    pub fn new(ingest: Arc<Ingestion<S>>) -> Self {
        Self { ingest }
    }
}

pub fn metrics_server<S: Stack>(
    ingest: Arc<Ingestion<S>>,
) -> MetricsServiceServer<MetricsGateway<S>> {
    MetricsServiceServer::new(MetricsGateway::new(ingest))
        .accept_compressed(CompressionEncoding::Gzip)
}

impl<S: Stack> fmt::Debug for MetricsGateway<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MetricsGateway")
            .finish_non_exhaustive()
    }
}

#[tonic::async_trait]
impl<S: Stack> MetricsService for MetricsGateway<S> {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        let key = auth::key_of(request.metadata())?;
        let message = request.into_inner();
        let decompressed = DecompressedBytes::new(message.encoded_len() as u64);
        let metrics = metrics_of(message).map_err(ApiError::from)?;
        self.ingest
            .accept_metrics(
                &key,
                MetricPayload {
                    metrics,
                    decompressed,
                },
            )
            .await
            .map_err(ApiError::from)?;
        Ok(Response::new(ExportMetricsServiceResponse::default()))
    }
}
