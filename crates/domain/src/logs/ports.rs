use core::future::Future;

use super::value_objects::{Buckets, LogBucket, LogPage, LogQuery};
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait LogReader {
    fn search(
        &self,
        org: TenantId,
        query: &LogQuery,
    ) -> impl Future<Output = Result<LogPage, StoreError>> + Send;

    fn histogram(
        &self,
        org: TenantId,
        query: &LogQuery,
        buckets: Buckets,
    ) -> impl Future<Output = Result<Vec<LogBucket>, StoreError>> + Send;
}

impl<T: LogReader + Sync> LogReader for &T {
    fn search(
        &self,
        org: TenantId,
        query: &LogQuery,
    ) -> impl Future<Output = Result<LogPage, StoreError>> + Send {
        (*self).search(org, query)
    }

    fn histogram(
        &self,
        org: TenantId,
        query: &LogQuery,
        buckets: Buckets,
    ) -> impl Future<Output = Result<Vec<LogBucket>, StoreError>> + Send {
        (*self).histogram(org, query, buckets)
    }
}

impl<T: LogReader + Send + Sync> LogReader for std::sync::Arc<T> {
    fn search(
        &self,
        org: TenantId,
        query: &LogQuery,
    ) -> impl Future<Output = Result<LogPage, StoreError>> + Send {
        self.as_ref().search(org, query)
    }

    fn histogram(
        &self,
        org: TenantId,
        query: &LogQuery,
        buckets: Buckets,
    ) -> impl Future<Output = Result<Vec<LogBucket>, StoreError>> + Send {
        self.as_ref().histogram(org, query, buckets)
    }
}
