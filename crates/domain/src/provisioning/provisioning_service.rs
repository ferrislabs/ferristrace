use super::errors::ProvisionError;
use super::ports::ControlPlane;
use super::value_objects::{Bootstrapped, DEFAULT_PROJECT, Founded, Issued};
use crate::access::KeyScope;
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::catalogue::{ProjectId, ProjectName};
use crate::identity::{AccountId, Email};
use crate::organisation::{OrgName, OrgRole};
use crate::secret::ResetToken;
use crate::telemetry::TenantId;

#[derive(Debug, Clone)]
pub struct Provisioning<C, A> {
    control_plane: C,
    trail: A,
}

impl<C: ControlPlane, A: AuditTrail> Provisioning<C, A> {
    pub fn new(control_plane: C, trail: A) -> Self {
        Self {
            control_plane,
            trail,
        }
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
        self.note_founding(org, name, owner).await?;

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
        let token = self.invite(account).await?;
        for org in self.control_plane.memberships_of(account).await? {
            self.note(
                org,
                Action::ResetLinkIssuedByOperator,
                Target::account(account),
                Detail::default().with("email", email.as_str()),
            )
            .await?;
        }
        Ok(token)
    }

    pub async fn create_org(
        &self,
        name: &OrgName,
        owner: &Email,
    ) -> Result<TenantId, ProvisionError> {
        let org = self.control_plane.found(name, owner).await?;
        self.note_founding(org, name, owner).await?;
        Ok(org)
    }

    pub async fn grant_role(
        &self,
        org: TenantId,
        member: &Email,
        role: OrgRole,
    ) -> Result<(), ProvisionError> {
        self.control_plane.grant(org, member, role).await?;
        let account = self.control_plane.account_for(member).await?;
        let target = account.map_or_else(|| Target::of(TargetKind::Member, None), Target::member);
        self.note(
            org,
            Action::MemberRoleGranted,
            target,
            Detail::default()
                .with("email", member.as_str())
                .with("to", role),
        )
        .await
    }

    pub async fn create_project(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<ProjectId, ProvisionError> {
        let project = self.control_plane.claim_project(org, name).await?;
        self.note(
            org,
            Action::ProjectCreated,
            Target::project(project),
            Detail::default().with("name", name.as_str()),
        )
        .await?;
        Ok(project)
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
        let minted = self.control_plane.issue_key(org, project).await?;
        self.note(
            org,
            Action::KeyIssued,
            Target::key(minted.id),
            Detail::default()
                .with("scopes", KeyScope::Ingest)
                .with_maybe("project", project),
        )
        .await?;
        Ok(Issued {
            key: minted.key,
            project,
        })
    }

    pub async fn revoke_key(&self, prefix: &str) -> Result<bool, ProvisionError> {
        let Some(revoked) = self.control_plane.revoke_key(prefix).await? else {
            return Ok(false);
        };
        self.note(
            revoked.org,
            Action::KeyRevoked,
            Target::key(revoked.id),
            Detail::default().with("prefix", prefix),
        )
        .await?;
        Ok(true)
    }

    async fn note_founding(
        &self,
        org: TenantId,
        name: &OrgName,
        owner: &Email,
    ) -> Result<(), ProvisionError> {
        self.note(
            org,
            Action::OrgFounded,
            Target::org(org),
            Detail::default()
                .with("name", name.as_str())
                .with("owner", owner.as_str()),
        )
        .await
    }

    async fn note(
        &self,
        org: TenantId,
        action: Action,
        target: Target,
        detail: Detail,
    ) -> Result<(), ProvisionError> {
        self.trail
            .record(&AuditEntry {
                org,
                actor: Actor::Operator,
                action,
                target,
                detail,
            })
            .await
            .map_err(|_| ProvisionError::Unreachable)
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
