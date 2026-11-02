use core::fmt;
use std::time::Duration;

use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, Clock, ControlPlane, Email, IngestKey, OrgName, OrgRole, ProjectId, ProjectName,
    ProvisionError, ResetStore, SecretHash, TenantId,
};
use ferristrace_infra::Argon2Hasher;

use crate::accounts::{AccountError, PostgresAccounts};
use crate::keys::{issue_on, revoke_on};
use crate::memberships::{MembershipError, PostgresMemberships};
use crate::projects::{PostgresProjects, ProjectError};
use crate::resets::PostgresResets;
use crate::unit_of_work::{Postgres, SharedTx};

pub const INVITE_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);

#[repository(domain = ControlPlane, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresControlPlane<C> {
    tx: SharedTx,
    clock: C,
    hasher: Argon2Hasher,
    memberships: PostgresMemberships,
    projects: PostgresProjects,
    accounts: PostgresAccounts,
    resets: PostgresResets<C>,
}

impl<C: Clock + Clone> PostgresControlPlane<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            clock: backend.clock().clone(),
            hasher: backend.hasher.clone(),
            memberships: PostgresMemberships::new(backend, tx),
            projects: PostgresProjects::new(backend, tx),
            accounts: PostgresAccounts::new(backend, tx),
            resets: PostgresResets::new(backend, tx).with_window(backend.invite_window),
        }
    }
}

impl<C: Clock + Clone + Send + Sync> ControlPlane for PostgresControlPlane<C> {
    async fn founded(&self) -> Result<Option<TenantId>, ProvisionError> {
        self.memberships.founded().await.map_err(of_membership)
    }

    async fn found(&self, name: &OrgName, owner: &Email) -> Result<TenantId, ProvisionError> {
        self.memberships
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
        self.memberships
            .grant(org, member.as_str(), role)
            .await
            .map_err(of_membership)
    }

    async fn account_for(&self, email: &Email) -> Result<Option<AccountId>, ProvisionError> {
        self.resets
            .account_for(email)
            .await
            .map_err(|error| unreachable(&error))
    }

    async fn create_locked_account(&self, email: &Email) -> Result<AccountId, ProvisionError> {
        let locked = self.hasher.locked().map_err(|error| unreachable(&error))?;
        self.accounts
            .create(email, &locked)
            .await
            .map_err(of_account)
    }

    async fn invite(&self, account: AccountId, digest: SecretHash) -> Result<(), ProvisionError> {
        self.resets
            .begin(account, digest)
            .await
            .map_err(|error| unreachable(&error))
    }

    async fn claim_project(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<ProjectId, ProvisionError> {
        self.projects.claim(org, name).await.map_err(of_project)
    }

    async fn only_project(&self, org: TenantId) -> Result<Option<ProjectId>, ProvisionError> {
        self.projects.only(org).await.map_err(of_project)
    }

    async fn issue_key(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
    ) -> Result<IngestKey, ProvisionError> {
        let mut tx = self.tx.lock().await;
        issue_on(&mut tx, org, project.map(ProjectId::as_uuid))
            .await
            .map_err(|error| unreachable(&error))
    }

    async fn revoke_key(&self, prefix: &str) -> Result<bool, ProvisionError> {
        let mut tx = self.tx.lock().await;
        revoke_on(&mut tx, prefix, self.clock.now())
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
