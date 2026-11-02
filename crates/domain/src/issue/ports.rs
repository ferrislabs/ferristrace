use core::future::Future;

use super::value_objects::{Counted, Fingerprint, IssueFilter, Occurrence, Recorded, Status};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::identity::AccountId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait IssueReader {
    fn issues(
        &self,
        tenant: TenantId,
        filter: &IssueFilter,
    ) -> impl Future<Output = Result<Vec<Counted>, StoreError>> + Send;

    fn occurrences(
        &self,
        tenant: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Occurrence>, StoreError>> + Send;
}

impl<T: IssueReader + Sync> IssueReader for &T {
    fn issues(
        &self,
        tenant: TenantId,
        filter: &IssueFilter,
    ) -> impl Future<Output = Result<Vec<Counted>, StoreError>> + Send {
        (*self).issues(tenant, filter)
    }

    fn occurrences(
        &self,
        tenant: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Occurrence>, StoreError>> + Send {
        (*self).occurrences(tenant, project, print, window, limit)
    }
}

pub trait IssueDecisions {
    fn read_many(
        &self,
        of: &[(ProjectId, Fingerprint)],
    ) -> impl Future<Output = Result<Vec<Recorded>, StoreError>> + Send;

    fn decide(
        &self,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn assign(
        &self,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl<T: IssueDecisions + Sync> IssueDecisions for &T {
    fn read_many(
        &self,
        of: &[(ProjectId, Fingerprint)],
    ) -> impl Future<Output = Result<Vec<Recorded>, StoreError>> + Send {
        (*self).read_many(of)
    }

    fn decide(
        &self,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).decide(project, print, status, by, at_unix_nanos)
    }

    fn assign(
        &self,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).assign(project, print, owner, by, at_unix_nanos)
    }
}
