use std::collections::BTreeMap;

use ferristrace_domain::{Admission, DecompressedBytes, QuotaPolicy, TenantId, UsageReader};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allowance {
    Unlimited,
    Capped(DecompressedBytes),
    Overage(DecompressedBytes),
}

#[derive(Debug, Default)]
pub struct StaticQuota<R> {
    allowances: BTreeMap<TenantId, Allowance>,
    usage: R,
}

impl<R> StaticQuota<R> {
    pub fn new(usage: R, allowances: impl IntoIterator<Item = (TenantId, Allowance)>) -> Self {
        Self {
            allowances: allowances.into_iter().collect(),
            usage,
        }
    }

    pub fn unlimited(usage: R) -> Self {
        Self {
            allowances: BTreeMap::new(),
            usage,
        }
    }
}

impl<R: UsageReader + Send + Sync> QuotaPolicy for StaticQuota<R> {
    async fn admit(&self, tenant: TenantId, incoming: DecompressedBytes) -> Admission {
        let Some(allowance) = self.allowances.get(&tenant).copied() else {
            return Admission::Within;
        };
        if allowance == Allowance::Unlimited {
            return Admission::Within;
        }

        let total = self
            .usage
            .used(tenant)
            .await
            .get()
            .saturating_add(incoming.get());

        match allowance {
            Allowance::Unlimited => Admission::Within,
            Allowance::Capped(limit) | Allowance::Overage(limit) if total <= limit.get() => {
                Admission::Within
            }
            Allowance::Capped(limit) => Admission::Capped { allowance: limit },
            Allowance::Overage(limit) => Admission::Overage {
                over_by: DecompressedBytes::new(total - limit.get()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{Admission, DecompressedBytes, QuotaPolicy, TenantId, UsageMeter};
    use uuid::Uuid;

    use super::{Allowance, StaticQuota};
    use crate::MemoryMeter;

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
        meter.record_ingest(tenant, bytes(900)).await;
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
        meter.record_ingest(tenant, bytes(900)).await;
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
}
