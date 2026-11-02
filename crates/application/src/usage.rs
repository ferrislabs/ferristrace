use core::future::Future;

use ferristrace_domain::telemetry::{
    BillingHour, BillingMonth, HourTotal, HourlyCounters, MonthTotal, UsageLedger, UsageSyncError,
};
use ferristrace_domain::{
    Clock, Consumption, ExploreError, Metering, SessionToken, StoreError, TenantId, Whoami, Window,
};

use crate::registry::{RepoFor, UsageRepos, domain};
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: UsageRepos,
{
    #[transactional(sessions, memberships, usage_history, unavailable = ExploreError::StoreUnreachable)]
    pub async fn consumption(
        &self,
        token: &SessionToken,
        org: TenantId,
        window: Window,
    ) -> Result<Consumption, ExploreError> {
        Metering::new(
            Whoami::new(sessions, memberships),
            &self.meter,
            usage_history,
        )
        .consumption(token, org, window)
        .await
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: RepoFor<domain::UsageHistory, Repo: UsageLedger>,
    S::Meter: HourlyCounters,
{
    pub fn roll_up_usage(&self) -> impl Future<Output = Result<usize, UsageSyncError>> + Send {
        async move {
            let hour = BillingHour::containing(self.clock.now()).previous();
            let finished = self.meter.finished(hour).await?;
            let rolled = self.record_hours(&finished).await?;
            for total in finished {
                self.meter.discard(total).await?;
            }
            Ok(rolled)
        }
    }

    pub fn rehydrate_usage(&self) -> impl Future<Output = Result<usize, UsageSyncError>> + Send {
        async move {
            let month = BillingMonth::containing(self.clock.now());
            let billed = self.billed_since(month).await?;
            let mut seeded = 0;
            for total in billed {
                if self.meter.seed(total.tenant, total.ingested).await? {
                    seeded += 1;
                }
            }
            Ok(seeded)
        }
    }

    #[transactional(usage_history, unavailable = UsageSyncError::Store(StoreError::Unreachable))]
    async fn record_hours(&self, hours: &[HourTotal]) -> Result<usize, UsageSyncError> {
        Ok(usage_history.record_hours(hours).await?)
    }

    #[transactional(usage_history, unavailable = UsageSyncError::Store(StoreError::Unreachable))]
    async fn billed_since(&self, month: BillingMonth) -> Result<Vec<MonthTotal>, UsageSyncError> {
        Ok(usage_history.billed_since(month).await?)
    }
}
