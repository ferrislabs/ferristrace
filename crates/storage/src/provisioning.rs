use core::fmt;
use std::time::Duration;

use ferristrace_domain::{
    AccountId, ControlPlane, Email, HashError, IngestKey, OrgName, OrgRole, ProjectId, ProjectName,
    ProvisionError, ResetStore, SecretHash, TenantId,
};
use sqlx::PgPool;

use crate::accounts::{AccountError, PostgresAccounts};
use crate::clock::SystemClock;
use crate::hasher::Argon2Hasher;
use crate::keys::PostgresDirectory;
use crate::memberships::{MembershipError, PostgresMemberships};
use crate::projects::{PostgresProjects, ProjectError};
use crate::resets::PostgresResets;

pub const INVITE_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
pub struct PostgresControlPlane {
    pool: PgPool,
    hasher: Argon2Hasher,
    window: Duration,
}

impl fmt::Debug for PostgresControlPlane {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PostgresControlPlane")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

impl PostgresControlPlane {
    pub fn new(pool: PgPool) -> Result<Self, HashError> {
        Ok(Self {
            pool,
            hasher: Argon2Hasher::new()?,
            window: INVITE_WINDOW,
        })
    }

    fn memberships(&self) -> PostgresMemberships {
        PostgresMemberships::new(self.pool.clone())
    }

    fn projects(&self) -> PostgresProjects {
        PostgresProjects::new(self.pool.clone())
    }

    fn keys(&self) -> PostgresDirectory<SystemClock> {
        PostgresDirectory::new(self.pool.clone(), SystemClock)
    }

    fn resets(&self) -> PostgresResets<SystemClock> {
        PostgresResets::new(self.pool.clone(), SystemClock).with_window(self.window)
    }
}

impl ControlPlane for PostgresControlPlane {
    async fn founded(&self) -> Result<Option<TenantId>, ProvisionError> {
        self.memberships().founded().await.map_err(of_membership)
    }

    async fn found(&self, name: &OrgName, owner: &Email) -> Result<TenantId, ProvisionError> {
        self.memberships()
            .found(name.as_str(), owner.as_str())
            .await
            .map_err(of_membership)
    }

    async fn grant(
        &self,
        org: TenantId,
        member: &Email,
        role: OrgRole,
    ) -> Result<(), ProvisionError> {
        self.memberships()
            .grant(org, member.as_str(), role)
            .await
            .map_err(of_membership)
    }

    async fn account_for(&self, email: &Email) -> Result<Option<AccountId>, ProvisionError> {
        self.resets()
            .account_for(email)
            .await
            .map_err(|error| unreachable(&error))
    }

    async fn create_locked_account(&self, email: &Email) -> Result<AccountId, ProvisionError> {
        let locked = self.hasher.locked().map_err(|error| unreachable(&error))?;
        PostgresAccounts::new(self.pool.clone())
            .create(email, &locked)
            .await
            .map_err(of_account)
    }

    async fn invite(&self, account: AccountId, digest: SecretHash) -> Result<(), ProvisionError> {
        self.resets()
            .begin(account, digest)
            .await
            .map_err(|error| unreachable(&error))
    }

    async fn claim_project(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<ProjectId, ProvisionError> {
        self.projects().claim(org, name).await.map_err(of_project)
    }

    async fn only_project(&self, org: TenantId) -> Result<Option<ProjectId>, ProvisionError> {
        self.projects().only(org).await.map_err(of_project)
    }

    async fn issue_key(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
    ) -> Result<IngestKey, ProvisionError> {
        self.keys()
            .issue(org, project.map(ProjectId::as_uuid))
            .await
            .map_err(|error| unreachable(&error))
    }

    async fn revoke_key(&self, prefix: &str) -> Result<bool, ProvisionError> {
        self.keys()
            .revoke(prefix)
            .await
            .map_err(|error| unreachable(&error))
    }
}

fn of_membership(error: MembershipError) -> ProvisionError {
    match error {
        MembershipError::NoSuchAccount(_) => ProvisionError::NoSuchAccount,
        other => unreachable(&other),
    }
}

fn of_project(error: ProjectError) -> ProvisionError {
    match error {
        ProjectError::Taken(_) => ProvisionError::Taken,
        ProjectError::NoSuchOrg(_) => ProvisionError::NoSuchOrg,
        other @ ProjectError::ControlPlane(_) => unreachable(&other),
    }
}

fn of_account(error: AccountError) -> ProvisionError {
    match error {
        AccountError::Taken => ProvisionError::Taken,
        other => unreachable(&other),
    }
}

fn unreachable<E: fmt::Display>(error: &E) -> ProvisionError {
    tracing::error!(%error, "the control plane refused a provisioning write");
    ProvisionError::Unreachable
}
