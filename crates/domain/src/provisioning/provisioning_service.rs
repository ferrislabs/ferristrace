use super::errors::ProvisionError;
use super::ports::ControlPlane;
use super::value_objects::{Bootstrapped, DEFAULT_PROJECT, Founded, Issued};
use crate::catalogue::{ProjectId, ProjectName};
use crate::identity::{AccountId, Email};
use crate::organisation::{OrgName, OrgRole};
use crate::secret::ResetToken;
use crate::telemetry::TenantId;

#[derive(Debug, Clone)]
pub struct Provisioning<C> {
    control_plane: C,
}

impl<C: ControlPlane> Provisioning<C> {
    pub fn new(control_plane: C) -> Self {
        Self { control_plane }
    }

    pub async fn bootstrap(
        &self,
        name: &OrgName,
        owner: &Email,
        local_password: bool,
    ) -> Result<Bootstrapped, ProvisionError> {
        if let Some(existing) = self.control_plane.founded().await? {
            return Ok(Bootstrapped::AlreadyFounded { existing });
        }

        let account = self.account_for_or_create(owner).await?;
        let org = self.control_plane.found(name, owner).await?;
        let project_name = default_project();
        let project = self.control_plane.claim_project(org, &project_name).await?;
        let invitation = if local_password {
            Some(self.invite(account).await?)
        } else {
            None
        };

        Ok(Bootstrapped::Founded(Founded {
            org,
            project,
            project_name,
            invitation,
        }))
    }

    pub async fn create_account(&self, email: &Email) -> Result<ResetToken, ProvisionError> {
        let account = self.control_plane.create_locked_account(email).await?;
        self.invite(account).await
    }

    pub async fn reset_password(&self, email: &Email) -> Result<ResetToken, ProvisionError> {
        let account = self
            .control_plane
            .account_for(email)
            .await?
            .ok_or(ProvisionError::NoSuchAccount)?;
        self.invite(account).await
    }

    pub async fn create_org(
        &self,
        name: &OrgName,
        owner: &Email,
    ) -> Result<TenantId, ProvisionError> {
        self.control_plane.found(name, owner).await
    }

    pub async fn grant_role(
        &self,
        org: TenantId,
        member: &Email,
        role: OrgRole,
    ) -> Result<(), ProvisionError> {
        self.control_plane.grant(org, member, role).await
    }

    pub async fn create_project(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<ProjectId, ProvisionError> {
        self.control_plane.claim_project(org, name).await
    }

    pub async fn issue_key(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
    ) -> Result<Issued, ProvisionError> {
        let project = match project {
            Some(named) => Some(named),
            None => self.control_plane.only_project(org).await?,
        };
        let key = self.control_plane.issue_key(org, project).await?;
        Ok(Issued { key, project })
    }

    pub async fn revoke_key(&self, prefix: &str) -> Result<bool, ProvisionError> {
        self.control_plane.revoke_key(prefix).await
    }

    async fn account_for_or_create(&self, email: &Email) -> Result<AccountId, ProvisionError> {
        if let Some(found) = self.control_plane.account_for(email).await? {
            return Ok(found);
        }
        self.control_plane.create_locked_account(email).await
    }

    async fn invite(&self, account: AccountId) -> Result<ResetToken, ProvisionError> {
        let token = ResetToken::random();
        self.control_plane.invite(account, token.hash()).await?;
        Ok(token)
    }
}

fn default_project() -> ProjectName {
    ProjectName::try_from(DEFAULT_PROJECT).expect("a slug")
}
