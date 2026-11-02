use core::future::Future;

use super::entities::{Snapshot, SpanCursor, SpanPage};
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait ExportSource {
    fn snapshot(&self, org: TenantId) -> impl Future<Output = Result<Snapshot, StoreError>> + Send;
}

impl<T: ExportSource + Sync> ExportSource for &T {
    fn snapshot(&self, org: TenantId) -> impl Future<Output = Result<Snapshot, StoreError>> + Send {
        (*self).snapshot(org)
    }
}

impl<T: ExportSource + Send + Sync> ExportSource for std::sync::Arc<T> {
    fn snapshot(&self, org: TenantId) -> impl Future<Output = Result<Snapshot, StoreError>> + Send {
        self.as_ref().snapshot(org)
    }
}

pub trait SpanPages {
    fn page(
        &self,
        org: TenantId,
        after: Option<&SpanCursor>,
        limit: u32,
    ) -> impl Future<Output = Result<SpanPage, StoreError>> + Send;
}

impl<T: SpanPages + Sync> SpanPages for &T {
    fn page(
        &self,
        org: TenantId,
        after: Option<&SpanCursor>,
        limit: u32,
    ) -> impl Future<Output = Result<SpanPage, StoreError>> + Send {
        (*self).page(org, after, limit)
    }
}

impl<T: SpanPages + Send + Sync> SpanPages for std::sync::Arc<T> {
    fn page(
        &self,
        org: TenantId,
        after: Option<&SpanCursor>,
        limit: u32,
    ) -> impl Future<Output = Result<SpanPage, StoreError>> + Send {
        self.as_ref().page(org, after, limit)
    }
}
