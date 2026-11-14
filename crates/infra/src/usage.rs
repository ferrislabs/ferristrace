use core::future::Future;
use std::collections::BTreeMap;
use std::sync::Mutex;

use ferristrace_domain::{Carried, DecompressedBytes, TenantId, UsageMeter, UsageReader};

#[derive(Debug, Default)]
pub struct MemoryMeter {
    totals: Mutex<BTreeMap<TenantId, u64>>,
}

impl MemoryMeter {
    pub fn total_for(&self, tenant: TenantId) -> u64 {
        self.totals
            .lock()
            .expect("meter totals")
            .get(&tenant)
            .copied()
            .unwrap_or_default()
    }

    pub fn tenants(&self) -> Vec<TenantId> {
        self.totals
            .lock()
            .expect("meter totals")
            .keys()
            .copied()
            .collect()
    }
}

impl UsageMeter for MemoryMeter {
    fn record_ingest(
        &self,
        tenant: TenantId,
        decompressed: DecompressedBytes,
        _carried: Carried,
    ) -> impl Future<Output = ()> + Send {
        *self
            .totals
            .lock()
            .expect("meter totals")
            .entry(tenant)
            .or_default() += decompressed.get();
        async {}
    }
}

impl UsageReader for MemoryMeter {
    fn used(&self, tenant: TenantId) -> impl Future<Output = DecompressedBytes> + Send {
        let total = DecompressedBytes::new(self.total_for(tenant));
        async move { total }
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{Carried, DecompressedBytes, TenantId, UsageMeter};
    use uuid::Uuid;

    use super::MemoryMeter;

    #[tokio::test]
    async fn readings_accumulate_per_tenant() {
        let first = TenantId::new(Uuid::from_u128(1));
        let second = TenantId::new(Uuid::from_u128(2));
        let meter = MemoryMeter::default();

        meter
            .record_ingest(first, DecompressedBytes::new(100), Carried::spans(100))
            .await;
        meter
            .record_ingest(first, DecompressedBytes::new(40), Carried::spans(40))
            .await;
        meter
            .record_ingest(second, DecompressedBytes::new(7), Carried::spans(7))
            .await;

        assert_eq!(meter.total_for(first), 140);
        assert_eq!(meter.total_for(second), 7);
        assert_eq!(meter.tenants(), vec![first, second]);
    }

    #[test]
    fn an_unseen_tenant_has_no_usage() {
        let meter = MemoryMeter::default();

        assert_eq!(meter.total_for(TenantId::new(Uuid::from_u128(9))), 0);
    }
}
