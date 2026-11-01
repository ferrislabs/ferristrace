use std::sync::Arc;

use ferristrace_domain::{
    DecompressedBytes, Ingest, Payload, QuotaPolicy, SpanSink, TenantDirectory, UsageMeter,
};
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

#[derive(Debug)]
pub struct TraceGateway<D, Q, S, M> {
    ingest: Arc<Ingest<D, Q, S, M>>,
}

impl<D, Q, S, M> TraceGateway<D, Q, S, M> {
    pub fn new(ingest: Arc<Ingest<D, Q, S, M>>) -> Self {
        Self { ingest }
    }
}

pub fn trace_server<D, Q, S, M>(
    ingest: Arc<Ingest<D, Q, S, M>>,
) -> TraceServiceServer<TraceGateway<D, Q, S, M>>
where
    D: TenantDirectory + Send + Sync + 'static,
    Q: QuotaPolicy + Send + Sync + 'static,
    S: SpanSink + Send + Sync + 'static,
    M: UsageMeter + Send + Sync + 'static,
{
    TraceServiceServer::new(TraceGateway::new(ingest)).accept_compressed(CompressionEncoding::Gzip)
}

#[tonic::async_trait]
impl<D, Q, S, M> TraceService for TraceGateway<D, Q, S, M>
where
    D: TenantDirectory + Send + Sync + 'static,
    Q: QuotaPolicy + Send + Sync + 'static,
    S: SpanSink + Send + Sync + 'static,
    M: UsageMeter + Send + Sync + 'static,
{
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
