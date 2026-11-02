use core::future::Future;

use super::entities::Span;
use super::errors::{SinkError, UnknownKey};
use super::value_objects::{Admission, Consumed, DecompressedBytes, IngestKey, Scope, TenantId};
use crate::explore::{ReadError, Window};

pub trait SpanSink {
    fn accept(
        &self,
        scope: Scope,
        spans: Vec<Span>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send;
}

impl<T: SpanSink + Sync> SpanSink for &T {
    fn accept(
        &self,
        scope: Scope,
        spans: Vec<Span>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        (*self).accept(scope, spans)
    }
}

impl<T: SpanSink + Send + Sync> SpanSink for std::sync::Arc<T> {
    fn accept(
        &self,
        scope: Scope,
        spans: Vec<Span>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        self.as_ref().accept(scope, spans)
    }
}

pub trait TenantDirectory {
    fn resolve(&self, key: &IngestKey) -> impl Future<Output = Result<Scope, UnknownKey>> + Send;
}

impl<T: TenantDirectory + Sync> TenantDirectory for &T {
    fn resolve(&self, key: &IngestKey) -> impl Future<Output = Result<Scope, UnknownKey>> + Send {
        (*self).resolve(key)
    }
}

impl<T: TenantDirectory + Send + Sync> TenantDirectory for std::sync::Arc<T> {
    fn resolve(&self, key: &IngestKey) -> impl Future<Output = Result<Scope, UnknownKey>> + Send {
        self.as_ref().resolve(key)
    }
}

pub trait QuotaPolicy {
    fn admit(
        &self,
        tenant: TenantId,
        incoming: DecompressedBytes,
    ) -> impl Future<Output = Admission> + Send;
}

impl<T: QuotaPolicy + Sync> QuotaPolicy for &T {
    fn admit(
        &self,
        tenant: TenantId,
        incoming: DecompressedBytes,
    ) -> impl Future<Output = Admission> + Send {
        (*self).admit(tenant, incoming)
    }
}

impl<T: QuotaPolicy + Send + Sync> QuotaPolicy for std::sync::Arc<T> {
    fn admit(
        &self,
        tenant: TenantId,
        incoming: DecompressedBytes,
    ) -> impl Future<Output = Admission> + Send {
        self.as_ref().admit(tenant, incoming)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Unmetered;

impl QuotaPolicy for Unmetered {
    async fn admit(&self, _tenant: TenantId, _incoming: DecompressedBytes) -> Admission {
        Admission::Within
    }
}

pub trait UsageMeter {
    fn record_ingest(
        &self,
        tenant: TenantId,
        decompressed: DecompressedBytes,
    ) -> impl Future<Output = ()> + Send;
}

impl<T: UsageMeter + Sync> UsageMeter for &T {
    fn record_ingest(
        &self,
        tenant: TenantId,
        decompressed: DecompressedBytes,
    ) -> impl Future<Output = ()> + Send {
        (*self).record_ingest(tenant, decompressed)
    }
}

impl<T: UsageMeter + Send + Sync> UsageMeter for std::sync::Arc<T> {
    fn record_ingest(
        &self,
        tenant: TenantId,
        decompressed: DecompressedBytes,
    ) -> impl Future<Output = ()> + Send {
        self.as_ref().record_ingest(tenant, decompressed)
    }
}

pub trait UsageReader {
    fn used(&self, tenant: TenantId) -> impl Future<Output = DecompressedBytes> + Send;
}

impl<T: UsageReader + Sync> UsageReader for &T {
    fn used(&self, tenant: TenantId) -> impl Future<Output = DecompressedBytes> + Send {
        (*self).used(tenant)
    }
}

impl<T: UsageReader + Send + Sync> UsageReader for std::sync::Arc<T> {
    fn used(&self, tenant: TenantId) -> impl Future<Output = DecompressedBytes> + Send {
        self.as_ref().used(tenant)
    }
}

pub trait UsageHistory {
    fn hours(
        &self,
        org: TenantId,
        window: Window,
    ) -> impl Future<Output = Result<Vec<Consumed>, ReadError>> + Send;

    fn included(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<DecompressedBytes>, ReadError>> + Send;
}

impl<T: UsageHistory + Sync> UsageHistory for &T {
    fn hours(
        &self,
        org: TenantId,
        window: Window,
    ) -> impl Future<Output = Result<Vec<Consumed>, ReadError>> + Send {
        (*self).hours(org, window)
    }

    fn included(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<DecompressedBytes>, ReadError>> + Send {
        (*self).included(org)
    }
}

impl<T: UsageHistory + Send + Sync> UsageHistory for std::sync::Arc<T> {
    fn hours(
        &self,
        org: TenantId,
        window: Window,
    ) -> impl Future<Output = Result<Vec<Consumed>, ReadError>> + Send {
        self.as_ref().hours(org, window)
    }

    fn included(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<DecompressedBytes>, ReadError>> + Send {
        self.as_ref().included(org)
    }
}
