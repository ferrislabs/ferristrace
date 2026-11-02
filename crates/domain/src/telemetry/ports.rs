use core::future::Future;

use super::billing::{BillingHour, BillingMonth};
use super::entities::{LogRecord, Span};
use super::errors::{CounterUnreachable, SinkError, UnknownKey};
use super::value_objects::{
    Admission, Consumed, DecompressedBytes, HourTotal, IngestKey, MonthTotal, Scope, TenantId,
};
use crate::explore::Window;
use crate::store::StoreError;

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

pub trait LogSink {
    fn accept_logs(
        &self,
        scope: Scope,
        records: Vec<LogRecord>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send;
}

impl<T: LogSink + Sync> LogSink for &T {
    fn accept_logs(
        &self,
        scope: Scope,
        records: Vec<LogRecord>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        (*self).accept_logs(scope, records)
    }
}

impl<T: LogSink + Send + Sync> LogSink for std::sync::Arc<T> {
    fn accept_logs(
        &self,
        scope: Scope,
        records: Vec<LogRecord>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        self.as_ref().accept_logs(scope, records)
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
    ) -> impl Future<Output = Result<Vec<Consumed>, StoreError>> + Send;

    fn included(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<DecompressedBytes>, StoreError>> + Send;
}

impl<T: UsageHistory + Sync> UsageHistory for &T {
    fn hours(
        &self,
        org: TenantId,
        window: Window,
    ) -> impl Future<Output = Result<Vec<Consumed>, StoreError>> + Send {
        (*self).hours(org, window)
    }

    fn included(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<DecompressedBytes>, StoreError>> + Send {
        (*self).included(org)
    }
}

impl<T: UsageHistory + Send + Sync> UsageHistory for std::sync::Arc<T> {
    fn hours(
        &self,
        org: TenantId,
        window: Window,
    ) -> impl Future<Output = Result<Vec<Consumed>, StoreError>> + Send {
        self.as_ref().hours(org, window)
    }

    fn included(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<DecompressedBytes>, StoreError>> + Send {
        self.as_ref().included(org)
    }
}

pub trait HourlyCounters {
    fn finished(
        &self,
        hour: BillingHour,
    ) -> impl Future<Output = Result<Vec<HourTotal>, CounterUnreachable>> + Send;

    fn discard(
        &self,
        total: HourTotal,
    ) -> impl Future<Output = Result<(), CounterUnreachable>> + Send;

    fn seed(
        &self,
        tenant: TenantId,
        total: DecompressedBytes,
    ) -> impl Future<Output = Result<bool, CounterUnreachable>> + Send;
}

impl<T: HourlyCounters + Sync> HourlyCounters for &T {
    fn finished(
        &self,
        hour: BillingHour,
    ) -> impl Future<Output = Result<Vec<HourTotal>, CounterUnreachable>> + Send {
        (*self).finished(hour)
    }

    fn discard(
        &self,
        total: HourTotal,
    ) -> impl Future<Output = Result<(), CounterUnreachable>> + Send {
        (*self).discard(total)
    }

    fn seed(
        &self,
        tenant: TenantId,
        total: DecompressedBytes,
    ) -> impl Future<Output = Result<bool, CounterUnreachable>> + Send {
        (*self).seed(tenant, total)
    }
}

impl<T: HourlyCounters + Send + Sync> HourlyCounters for std::sync::Arc<T> {
    fn finished(
        &self,
        hour: BillingHour,
    ) -> impl Future<Output = Result<Vec<HourTotal>, CounterUnreachable>> + Send {
        self.as_ref().finished(hour)
    }

    fn discard(
        &self,
        total: HourTotal,
    ) -> impl Future<Output = Result<(), CounterUnreachable>> + Send {
        self.as_ref().discard(total)
    }

    fn seed(
        &self,
        tenant: TenantId,
        total: DecompressedBytes,
    ) -> impl Future<Output = Result<bool, CounterUnreachable>> + Send {
        self.as_ref().seed(tenant, total)
    }
}

pub trait UsageLedger {
    fn record_hours(
        &self,
        hours: &[HourTotal],
    ) -> impl Future<Output = Result<usize, StoreError>> + Send;

    fn billed_since(
        &self,
        month: BillingMonth,
    ) -> impl Future<Output = Result<Vec<MonthTotal>, StoreError>> + Send;
}

impl<T: UsageLedger + Sync> UsageLedger for &T {
    fn record_hours(
        &self,
        hours: &[HourTotal],
    ) -> impl Future<Output = Result<usize, StoreError>> + Send {
        (*self).record_hours(hours)
    }

    fn billed_since(
        &self,
        month: BillingMonth,
    ) -> impl Future<Output = Result<Vec<MonthTotal>, StoreError>> + Send {
        (*self).billed_since(month)
    }
}

impl<T: UsageLedger + Send + Sync> UsageLedger for std::sync::Arc<T> {
    fn record_hours(
        &self,
        hours: &[HourTotal],
    ) -> impl Future<Output = Result<usize, StoreError>> + Send {
        self.as_ref().record_hours(hours)
    }

    fn billed_since(
        &self,
        month: BillingMonth,
    ) -> impl Future<Output = Result<Vec<MonthTotal>, StoreError>> + Send {
        self.as_ref().billed_since(month)
    }
}
