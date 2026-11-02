use core::future::Future;

use super::entities::{CallPage, Price, SeenModel};
use super::value_objects::{CallQuery, PriceDraft, PriceId};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::identity::AccountId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait LlmReader {
    fn calls(
        &self,
        org: TenantId,
        query: &CallQuery,
    ) -> impl Future<Output = Result<CallPage, StoreError>> + Send;

    fn models(
        &self,
        org: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Vec<SeenModel>, StoreError>> + Send;
}

pub trait ModelPrices {
    fn list(&self, org: TenantId) -> impl Future<Output = Result<Vec<Price>, StoreError>> + Send;

    fn create(
        &self,
        org: TenantId,
        draft: &PriceDraft,
        by: AccountId,
    ) -> impl Future<Output = Result<Option<Price>, StoreError>> + Send;

    fn delete(
        &self,
        org: TenantId,
        id: PriceId,
    ) -> impl Future<Output = Result<Option<Price>, StoreError>> + Send;
}
