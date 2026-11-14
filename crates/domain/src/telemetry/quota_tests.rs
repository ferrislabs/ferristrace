use std::collections::BTreeMap;
use std::sync::Mutex;

use uuid::Uuid;

use super::ports::{QuotaPolicy, UsageMeter, UsageReader};
use super::quota::{Allowance, StaticQuota};
use super::value_objects::{Admission, Carried, DecompressedBytes, TenantId};

#[derive(Debug, Default)]
struct MemoryMeter(Mutex<BTreeMap<TenantId, u64>>);

impl UsageMeter for MemoryMeter {
    fn record_ingest(
        &self,
        tenant: TenantId,
        decompressed: DecompressedBytes,
        _carried: Carried,
    ) -> impl core::future::Future<Output = ()> + Send {
        *self
            .0
            .lock()
            .expect("meter totals")
            .entry(tenant)
            .or_default() += decompressed.get();
        async {}
    }
}

impl UsageReader for MemoryMeter {
    fn used(
        &self,
        tenant: TenantId,
    ) -> impl core::future::Future<Output = DecompressedBytes> + Send {
        let total = self
            .0
            .lock()
            .expect("meter totals")
            .get(&tenant)
            .copied()
            .unwrap_or_default();
        async move { DecompressedBytes::new(total) }
    }
}

fn bytes(count: u64) -> DecompressedBytes {
    DecompressedBytes::new(count)
}

#[tokio::test]
async fn a_tenant_without_an_allowance_is_never_capped() {
    let tenant = TenantId::new(Uuid::from_u128(1));
    let quota = StaticQuota::unlimited(MemoryMeter::default());

    assert_eq!(
        quota.admit(tenant, bytes(u64::MAX)).await,
        Admission::Within
    );
}

#[tokio::test]
async fn a_hard_cap_refuses_the_payload_that_crosses_it() {
    let tenant = TenantId::new(Uuid::from_u128(2));
    let meter = MemoryMeter::default();
    meter
        .record_ingest(tenant, bytes(900), Carried::spans(900))
        .await;
    let quota = StaticQuota::new(&meter, [(tenant, Allowance::Capped(bytes(1000)))]);

    assert_eq!(quota.admit(tenant, bytes(50)).await, Admission::Within);
    assert_eq!(
        quota.admit(tenant, bytes(500)).await,
        Admission::Capped {
            allowance: bytes(1000)
        }
    );
}

#[tokio::test]
async fn an_overage_plan_reports_how_far_past_the_limit_it_goes() {
    let tenant = TenantId::new(Uuid::from_u128(3));
    let meter = MemoryMeter::default();
    meter
        .record_ingest(tenant, bytes(900), Carried::spans(900))
        .await;
    let quota = StaticQuota::new(&meter, [(tenant, Allowance::Overage(bytes(1000)))]);

    assert_eq!(
        quota.admit(tenant, bytes(500)).await,
        Admission::Overage {
            over_by: bytes(400)
        }
    );
}

#[tokio::test]
async fn one_tenant_allowance_does_not_bind_another() {
    let capped = TenantId::new(Uuid::from_u128(4));
    let free = TenantId::new(Uuid::from_u128(5));
    let quota = StaticQuota::new(
        MemoryMeter::default(),
        [(capped, Allowance::Capped(bytes(1)))],
    );

    assert_eq!(
        quota.admit(capped, bytes(2)).await,
        Admission::Capped {
            allowance: bytes(1)
        }
    );
    assert_eq!(quota.admit(free, bytes(u64::MAX)).await, Admission::Within);
}
