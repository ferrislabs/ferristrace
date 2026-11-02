use core::fmt;
use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, Clock, ControlPlane, Email, IngestKey, KeyId, Minted, OrgName, OrgRole, ProjectId,
    ProjectName, ProvisionError, Revoked, TenantId,
};

use crate::accounts::{AccountError, PostgresAccounts};
use crate::keys::{ingest_on, stop_on};
use crate::memberships::{MembershipError, PostgresMemberships};
use crate::projects::{PostgresProjects, ProjectError};
use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = ControlPlane, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresControlPlane<C> {
    tx: SharedTx,
    clock: C,
    memberships: PostgresMemberships,
    projects: PostgresProjects,
    accounts: PostgresAccounts,
}

impl<C: Clock + Clone> PostgresControlPlane<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            clock: backend.clock().clone(),
            memberships: PostgresMemberships::new(backend, tx),
            projects: PostgresProjects::new(backend, tx),
            accounts: PostgresAccounts::new(backend, tx),
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
        self.accounts
            .by_email(email)
            .await
            .map_err(|error| unreachable(&error))
    }

    async fn create_account(&self, email: &Email) -> Result<AccountId, ProvisionError> {
        self.accounts.create(email).await.map_err(of_account)
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
    ) -> Result<Minted, ProvisionError> {
        let mut tx = self.tx.lock().await;
        let minted = ingest_on(&mut tx, org, project.map(ProjectId::as_uuid))
            .await
            .map_err(|error| unreachable(&error))?;
        Ok(Minted {
            id: KeyId::new(minted.id),
            key: IngestKey::from_plaintext(&minted.plaintext),
        })
    }

    async fn revoke_key(&self, prefix: &str) -> Result<Option<Revoked>, ProvisionError> {
        let mut tx = self.tx.lock().await;
        let stopped = stop_on(&mut tx, prefix, self.clock.now())
            .await
            .map_err(|error| unreachable(&error))?;
        Ok(stopped.map(|stopped| Revoked {
            org: TenantId::new(stopped.org),
            id: KeyId::new(stopped.id),
        }))
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
