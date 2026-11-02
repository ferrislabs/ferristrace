use core::future::Future;

use super::entities::{ClaimedNotice, Notice, Origin, Override, Page, Schedule, ScheduleDraft};
use super::value_objects::{NoticeId, OverrideId, PageId, PageScope, ScheduleId};
use crate::alerts::Settlement;
use crate::organisation::TeamId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait Schedules {
    fn of_team(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<Vec<Schedule>, StoreError>> + Send;

    fn of_org(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Vec<Schedule>, StoreError>> + Send;

    fn get(
        &self,
        org: TenantId,
        id: ScheduleId,
    ) -> impl Future<Output = Result<Option<Schedule>, StoreError>> + Send;

    fn create(
        &self,
        org: TenantId,
        team: TeamId,
        draft: &ScheduleDraft,
    ) -> impl Future<Output = Result<Option<Schedule>, StoreError>> + Send;

    fn update(&self, schedule: &Schedule) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn delete(
        &self,
        org: TenantId,
        id: ScheduleId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn overrides(
        &self,
        org: TenantId,
        schedule: ScheduleId,
    ) -> impl Future<Output = Result<Vec<Override>, StoreError>> + Send;

    fn add_override(
        &self,
        org: TenantId,
        cover: &Override,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn remove_override(
        &self,
        org: TenantId,
        schedule: ScheduleId,
        id: OverrideId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
}

pub trait Pages {
    fn raise(&self, page: &Page) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn get(
        &self,
        org: TenantId,
        id: PageId,
    ) -> impl Future<Output = Result<Option<Page>, StoreError>> + Send;

    fn list(
        &self,
        org: TenantId,
        scope: PageScope,
    ) -> impl Future<Output = Result<Vec<Page>, StoreError>> + Send;

    fn due(
        &self,
        now_ms: u64,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Page>, StoreError>> + Send;

    fn keep(&self, page: &Page) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn resolve(
        &self,
        org: TenantId,
        origin: Origin,
        at_ms: u64,
    ) -> impl Future<Output = Result<Vec<Page>, StoreError>> + Send;

    fn enqueue(&self, notice: &Notice) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn claim(
        &self,
        now_ms: u64,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<ClaimedNotice>, StoreError>> + Send;

    fn settle(
        &self,
        id: NoticeId,
        attempts: u32,
        settlement: &Settlement,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}
