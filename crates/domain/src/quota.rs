use core::future::Future;

use crate::tenant::TenantId;
use crate::usage::DecompressedBytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Within,
    Overage { over_by: DecompressedBytes },
    Capped { allowance: DecompressedBytes },
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
