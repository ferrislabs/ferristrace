use core::future::Future;

use super::value_objects::{Counted, Fingerprint, IssueFilter, Occurrence, Recorded, Status};
use crate::catalogue::ProjectId;
use crate::explore::{ReadError, Window};
use crate::identity::AccountId;
use crate::telemetry::TenantId;

pub trait IssueReader {
    fn issues(
        &self,
        tenant: TenantId,
        filter: &IssueFilter,
    ) -> impl Future<Output = Result<Vec<Counted>, ReadError>> + Send;

    fn occurrences(
        &self,
        tenant: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Occurrence>, ReadError>> + Send;
}

impl<T: IssueReader + Sync> IssueReader for &T {
    fn issues(
        &self,
        tenant: TenantId,
        filter: &IssueFilter,
    ) -> impl Future<Output = Result<Vec<Counted>, ReadError>> + Send {
        (*self).issues(tenant, filter)
    }

    fn occurrences(
        &self,
        tenant: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Occurrence>, ReadError>> + Send {
        (*self).occurrences(tenant, project, print, window, limit)
    }
}

pub trait IssueDecisions {
    fn read_many(
        &self,
        of: &[(ProjectId, Fingerprint)],
    ) -> impl Future<Output = Result<Vec<Recorded>, ReadError>> + Send;

    fn decide(
        &self,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;

    fn assign(
        &self,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;
}

impl<T: IssueDecisions + Sync> IssueDecisions for &T {
    fn read_many(
        &self,
        of: &[(ProjectId, Fingerprint)],
    ) -> impl Future<Output = Result<Vec<Recorded>, ReadError>> + Send {
        (*self).read_many(of)
    }

    fn decide(
        &self,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).decide(project, print, status, by, at_unix_nanos)
    }

    fn assign(
        &self,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).assign(project, print, owner, by, at_unix_nanos)
    }
}
