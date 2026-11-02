use ferristrace_domain::{
    Ingest, IngestError, IngestKey, LogPayload, MetricPayload, Payload, TenantId,
};

use crate::Stack;

#[derive(Debug)]
pub struct Ingestion<S: Stack> {
    ingest: Ingest<S::Directory, S::Quota, S::Sink, S::Meter>,
}

impl<S: Stack> Ingestion<S> {
    pub fn new(directory: S::Directory, quota: S::Quota, sink: S::Sink, meter: S::Meter) -> Self {
        Self {
            ingest: Ingest::new(directory, quota, sink, meter),
        }
    }

    pub async fn accept(&self, key: &IngestKey, payload: Payload) -> Result<TenantId, IngestError> {
        self.ingest.accept(key, payload).await
    }

    pub async fn accept_logs(
        &self,
        key: &IngestKey,
        payload: LogPayload,
    ) -> Result<TenantId, IngestError> {
        self.ingest.accept_logs(key, payload).await
    }

    pub async fn accept_metrics(
        &self,
        key: &IngestKey,
        payload: MetricPayload,
    ) -> Result<TenantId, IngestError> {
        self.ingest.accept_metrics(key, payload).await
    }
}
