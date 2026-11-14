use super::errors::IngestError;
use super::ports::{LogSink, QuotaPolicy, SpanSink, TenantDirectory, UsageMeter};
use super::value_objects::{
    Admission, Carried, DecompressedBytes, IngestKey, LogPayload, Payload, Scope, TenantId,
};
use crate::metrics::{MetricPayload, MetricSink};

#[derive(Debug, Clone)]
pub struct Ingest<D, Q, S, M> {
    directory: D,
    quota: Q,
    sink: S,
    meter: M,
}

impl<D, Q, S, M> Ingest<D, Q, S, M>
where
    D: TenantDirectory,
    Q: QuotaPolicy,
    M: UsageMeter,
{
    pub fn new(directory: D, quota: Q, sink: S, meter: M) -> Self {
        Self {
            directory,
            quota,
            sink,
            meter,
        }
    }

    async fn admit(
        &self,
        key: &IngestKey,
        decompressed: DecompressedBytes,
    ) -> Result<Scope, IngestError> {
        let scope = self.directory.resolve(key).await?;
        if let Admission::Capped { allowance } = self.quota.admit(scope.tenant, decompressed).await
        {
            return Err(IngestError::Capped { allowance });
        }
        Ok(scope)
    }

    async fn metered(
        &self,
        scope: Scope,
        decompressed: DecompressedBytes,
        carried: Carried,
    ) -> TenantId {
        self.meter
            .record_ingest(scope.tenant, decompressed, carried)
            .await;
        scope.tenant
    }
}

impl<D, Q, S, M> Ingest<D, Q, S, M>
where
    D: TenantDirectory,
    Q: QuotaPolicy,
    S: SpanSink,
    M: UsageMeter,
{
    pub async fn accept(&self, key: &IngestKey, payload: Payload) -> Result<TenantId, IngestError> {
        let scope = self.admit(key, payload.decompressed).await?;
        self.sink.accept(scope, payload.spans).await?;
        let carried = Carried::spans(payload.decompressed.get());
        Ok(self.metered(scope, payload.decompressed, carried).await)
    }
}

impl<D, Q, S, M> Ingest<D, Q, S, M>
where
    D: TenantDirectory,
    Q: QuotaPolicy,
    S: LogSink,
    M: UsageMeter,
{
    pub async fn accept_logs(
        &self,
        key: &IngestKey,
        payload: LogPayload,
    ) -> Result<TenantId, IngestError> {
        let scope = self.admit(key, payload.decompressed).await?;
        self.sink.accept_logs(scope, payload.records).await?;
        let carried = Carried::logs(payload.decompressed.get());
        Ok(self.metered(scope, payload.decompressed, carried).await)
    }
}

impl<D, Q, S, M> Ingest<D, Q, S, M>
where
    D: TenantDirectory,
    Q: QuotaPolicy,
    S: MetricSink,
    M: UsageMeter,
{
    pub async fn accept_metrics(
        &self,
        key: &IngestKey,
        payload: MetricPayload,
    ) -> Result<TenantId, IngestError> {
        let scope = self.admit(key, payload.decompressed).await?;
        let carried = Carried::metrics(payload.datapoints());
        self.sink.accept_metrics(scope, payload.metrics).await?;
        Ok(self.metered(scope, payload.decompressed, carried).await)
    }
}
