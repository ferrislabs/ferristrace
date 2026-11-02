use core::future::Future;

use super::entities::{Attachment, Incident, IncidentFilter, TimelineEntry};
use super::value_objects::IncidentId;
use crate::alerts::RuleId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait Incidents {
    fn list(
        &self,
        org: TenantId,
        filter: &IncidentFilter,
    ) -> impl Future<Output = Result<Vec<Incident>, StoreError>> + Send;

    fn get(
        &self,
        org: TenantId,
        id: IncidentId,
    ) -> impl Future<Output = Result<Option<Incident>, StoreError>> + Send;

    fn open(&self, incident: &Incident) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn keep(&self, incident: &Incident) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn following(
        &self,
        org: TenantId,
        rule: RuleId,
    ) -> impl Future<Output = Result<Vec<IncidentId>, StoreError>> + Send;
}

pub trait IncidentTimeline {
    fn append(&self, entry: &TimelineEntry) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn append_once(
        &self,
        entry: &TimelineEntry,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn of(
        &self,
        org: TenantId,
        incident: IncidentId,
    ) -> impl Future<Output = Result<Vec<TimelineEntry>, StoreError>> + Send;
}

pub trait IncidentAttachments {
    fn attach(
        &self,
        org: TenantId,
        attachment: &Attachment,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn of(
        &self,
        org: TenantId,
        incident: IncidentId,
    ) -> impl Future<Output = Result<Vec<Attachment>, StoreError>> + Send;
}
