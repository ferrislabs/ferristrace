use core::future::Future;

use super::errors::ProvisionError;
use super::value_objects::{Minted, Revoked};
use crate::catalogue::{ProjectId, ProjectName};
use crate::identity::{AccountId, Email};
use crate::organisation::{OrgName, OrgRole};
use crate::secret::SecretHash;
use crate::telemetry::TenantId;

pub trait ControlPlane {
    fn founded(&self) -> impl Future<Output = Result<Option<TenantId>, ProvisionError>> + Send;

    fn found(
        &self,
        name: &OrgName,
        owner: &Email,
    ) -> impl Future<Output = Result<TenantId, ProvisionError>> + Send;

    fn grant(
        &self,
        org: TenantId,
        member: &Email,
        role: OrgRole,
    ) -> impl Future<Output = Result<(), ProvisionError>> + Send;

    fn account_for(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<Option<AccountId>, ProvisionError>> + Send;

    fn create_locked_account(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<AccountId, ProvisionError>> + Send;

    fn invite(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<(), ProvisionError>> + Send;

    fn claim_project(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<ProjectId, ProvisionError>> + Send;

    fn only_project(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<ProjectId>, ProvisionError>> + Send;

    fn issue_key(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
    ) -> impl Future<Output = Result<Minted, ProvisionError>> + Send;

    fn revoke_key(
        &self,
        prefix: &str,
    ) -> impl Future<Output = Result<Option<Revoked>, ProvisionError>> + Send;

    fn memberships_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<TenantId>, ProvisionError>> + Send;
}
