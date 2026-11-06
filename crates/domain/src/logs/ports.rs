use core::future::Future;

use super::value_objects::{Buckets, LogBucket, LogPage, LogQuery};
use crate::explore::{
    AttributeFilter, AttributeKey, FacetFilter, Facets, SeenAttribute, SeenValue,
};
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

    fn attributes(
        &self,
        _org: TenantId,
        _filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenAttribute>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn attribute_values(
        &self,
        _org: TenantId,
        _key: &AttributeKey,
        _filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenValue>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn facets(
        &self,
        _org: TenantId,
        _filter: &FacetFilter,
    ) -> impl Future<Output = Result<Facets, StoreError>> + Send {
        async { Ok(Facets::default()) }
    }
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

    fn attributes(
        &self,
        org: TenantId,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenAttribute>, StoreError>> + Send {
        (*self).attributes(org, filter)
    }

    fn attribute_values(
        &self,
        org: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenValue>, StoreError>> + Send {
        (*self).attribute_values(org, key, filter)
    }

    fn facets(
        &self,
        org: TenantId,
        filter: &FacetFilter,
    ) -> impl Future<Output = Result<Facets, StoreError>> + Send {
        (*self).facets(org, filter)
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

    fn attributes(
        &self,
        org: TenantId,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenAttribute>, StoreError>> + Send {
        self.as_ref().attributes(org, filter)
    }

    fn attribute_values(
        &self,
        org: TenantId,
        key: &AttributeKey,
        filter: &AttributeFilter,
    ) -> impl Future<Output = Result<Vec<SeenValue>, StoreError>> + Send {
        self.as_ref().attribute_values(org, key, filter)
    }

    fn facets(
        &self,
        org: TenantId,
        filter: &FacetFilter,
    ) -> impl Future<Output = Result<Facets, StoreError>> + Send {
        self.as_ref().facets(org, filter)
    }
}
