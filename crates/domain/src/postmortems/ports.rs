use core::future::Future;

use super::entities::{Postmortem, PostmortemSummary};
use crate::incidents::IncidentId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait Postmortems {
    fn of(
        &self,
        org: TenantId,
        incident: IncidentId,
    ) -> impl Future<Output = Result<Option<Postmortem>, StoreError>> + Send;

    fn summaries(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Vec<PostmortemSummary>, StoreError>> + Send;

    fn create(
        &self,
        postmortem: &Postmortem,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn keep(
        &self,
        postmortem: &Postmortem,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
}
