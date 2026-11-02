use super::errors::IngestError;
use super::ports::{QuotaPolicy, SpanSink, TenantDirectory, UsageMeter};
use super::value_objects::{Admission, IngestKey, Payload, TenantId};

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
    S: SpanSink,
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

    pub async fn accept(&self, key: &IngestKey, payload: Payload) -> Result<TenantId, IngestError> {
        let scope = self.directory.resolve(key).await?;
        if let Admission::Capped { allowance } =
            self.quota.admit(scope.tenant, payload.decompressed).await
        {
            return Err(IngestError::Capped { allowance });
        }
        self.sink.accept(scope, payload.spans).await?;
        self.meter
            .record_ingest(scope.tenant, payload.decompressed)
            .await;
        Ok(scope.tenant)
    }
}
