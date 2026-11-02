use ferristrace_domain::{
    Bootstrapped, Email, Issued, OrgName, OrgRole, ProjectId, ProjectName, ProvisionError,
    Provisioning, ResetToken, TenantId,
};

use crate::registry::ProvisioningRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: ProvisioningRepos,
{
    #[transactional(control_plane, unavailable = ProvisionError::Unreachable)]
    pub async fn bootstrap(
        &self,
        name: &OrgName,
        owner: &Email,
        local_password: bool,
    ) -> Result<Bootstrapped, ProvisionError> {
        Provisioning::new(control_plane)
            .bootstrap(name, owner, local_password)
            .await
    }

    #[transactional(control_plane, unavailable = ProvisionError::Unreachable)]
    pub async fn create_account(&self, email: &Email) -> Result<ResetToken, ProvisionError> {
        Provisioning::new(control_plane).create_account(email).await
    }

    #[transactional(control_plane, unavailable = ProvisionError::Unreachable)]
    pub async fn issue_reset_link(&self, email: &Email) -> Result<ResetToken, ProvisionError> {
        Provisioning::new(control_plane).reset_password(email).await
    }

    #[transactional(control_plane, unavailable = ProvisionError::Unreachable)]
    pub async fn create_org(
        &self,
        name: &OrgName,
        owner: &Email,
    ) -> Result<TenantId, ProvisionError> {
        Provisioning::new(control_plane)
            .create_org(name, owner)
            .await
    }

    #[transactional(control_plane, unavailable = ProvisionError::Unreachable)]
    pub async fn grant_org_role(
        &self,
        org: TenantId,
        member: &Email,
        role: OrgRole,
    ) -> Result<(), ProvisionError> {
        Provisioning::new(control_plane)
            .grant_role(org, member, role)
            .await
    }

    #[transactional(control_plane, unavailable = ProvisionError::Unreachable)]
    pub async fn claim_project(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<ProjectId, ProvisionError> {
        Provisioning::new(control_plane)
            .create_project(org, name)
            .await
    }

    #[transactional(control_plane, unavailable = ProvisionError::Unreachable)]
    pub async fn issue_ingest_key(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
    ) -> Result<Issued, ProvisionError> {
        Provisioning::new(control_plane)
            .issue_key(org, project)
            .await
    }

    #[transactional(control_plane, unavailable = ProvisionError::Unreachable)]
    pub async fn revoke_ingest_key(&self, prefix: &str) -> Result<bool, ProvisionError> {
        Provisioning::new(control_plane).revoke_key(prefix).await
    }
}
