use std::fmt;
use std::sync::Arc;

use ferristrace_application::{Ingestion, Stack};
use ferristrace_domain::{DecompressedBytes, Payload};
use ferristrace_otlp::spans_of;
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
