use std::collections::BTreeMap;

use super::ports::{QuotaPolicy, UsageReader};
use super::value_objects::{Admission, DecompressedBytes, TenantId};

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
