use core::future::Future;

use super::entities::{AuditEntry, AuditPage};
use super::value_objects::{AuditFilter, PageSize};
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait AuditTrail {
    fn record(&self, entry: &AuditEntry) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn list(
        &self,
        org: TenantId,
        filter: &AuditFilter,
        page: PageSize,
    ) -> impl Future<Output = Result<AuditPage, StoreError>> + Send;
}

impl<T: AuditTrail + Sync> AuditTrail for &T {
    fn record(&self, entry: &AuditEntry) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).record(entry)
    }

    fn list(
        &self,
        org: TenantId,
        filter: &AuditFilter,
        page: PageSize,
    ) -> impl Future<Output = Result<AuditPage, StoreError>> + Send {
        (*self).list(org, filter, page)
    }
}

impl<T: AuditTrail + Send + Sync> AuditTrail for std::sync::Arc<T> {
    fn record(&self, entry: &AuditEntry) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().record(entry)
    }

    fn list(
        &self,
        org: TenantId,
        filter: &AuditFilter,
        page: PageSize,
    ) -> impl Future<Output = Result<AuditPage, StoreError>> + Send {
        self.as_ref().list(org, filter, page)
    }
}
