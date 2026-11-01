use core::future::Future;

use crate::catalogue::{ProjectId, ProjectName};
use crate::credential::Email;
use crate::identity::AccountId;
use crate::key::IngestKey;
use crate::membership::OrgRole;
use crate::organisation::OrgName;
use crate::secret::{ResetToken, SecretHash};
use crate::tenant::TenantId;

pub const DEFAULT_PROJECT: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProvisionError {
    #[error("no account carries that address")]
    NoSuchAccount,
    #[error("no organisation carries that id")]
    NoSuchOrg,
    #[error("that name is already taken")]
    Taken,
    #[error("the control plane did not answer")]
    Unreachable,
}

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
    ) -> impl Future<Output = Result<IngestKey, ProvisionError>> + Send;

    fn revoke_key(&self, prefix: &str)
    -> impl Future<Output = Result<bool, ProvisionError>> + Send;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Founded {
    pub org: TenantId,
    pub project: ProjectId,
    pub project_name: ProjectName,
    pub invitation: Option<ResetToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bootstrapped {
    AlreadyFounded { existing: TenantId },
    Founded(Founded),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    pub key: IngestKey,
    pub project: Option<ProjectId>,
}

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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use uuid::Uuid;

    use super::{Bootstrapped, ControlPlane, Founded, ProvisionError, Provisioning};
    use crate::catalogue::{ProjectId, ProjectName};
    use crate::credential::Email;
    use crate::identity::AccountId;
    use crate::key::IngestKey;
    use crate::membership::OrgRole;
    use crate::organisation::OrgName;
    use crate::secret::SecretHash;
    use crate::tenant::TenantId;

    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct Written {
        org: Option<TenantId>,
        org_name: Option<OrgName>,
        accounts: Vec<(Email, AccountId)>,
        invited: Vec<AccountId>,
        projects: Vec<(TenantId, ProjectName, ProjectId)>,
        issued: Vec<(TenantId, Option<ProjectId>)>,
        granted: Vec<(TenantId, Email, OrgRole)>,
    }

    #[derive(Debug)]
    struct FakeControlPlane {
        written: Arc<Mutex<Written>>,
        unreachable: bool,
    }

    impl ControlPlane for FakeControlPlane {
        async fn founded(&self) -> Result<Option<TenantId>, ProvisionError> {
            if self.unreachable {
                return Err(ProvisionError::Unreachable);
            }
            Ok(self.written.lock().expect("the lock").org)
        }

        async fn found(&self, name: &OrgName, owner: &Email) -> Result<TenantId, ProvisionError> {
            let mut held = self.written.lock().expect("the lock");
            if !held.accounts.iter().any(|(carried, _)| carried == owner) {
                return Err(ProvisionError::NoSuchAccount);
            }
            let org = TenantId::new(Uuid::new_v4());
            held.org = Some(org);
            held.org_name = Some(name.clone());
            Ok(org)
        }

        async fn grant(
            &self,
            org: TenantId,
            member: &Email,
            role: OrgRole,
        ) -> Result<(), ProvisionError> {
            let mut held = self.written.lock().expect("the lock");
            if !held.accounts.iter().any(|(carried, _)| carried == member) {
                return Err(ProvisionError::NoSuchAccount);
            }
            held.granted.push((org, member.clone(), role));
            Ok(())
        }

        async fn account_for(&self, email: &Email) -> Result<Option<AccountId>, ProvisionError> {
            let held = self.written.lock().expect("the lock");
            Ok(held
                .accounts
                .iter()
                .find(|(carried, _)| carried == email)
                .map(|(_, account)| *account))
        }

        async fn create_locked_account(&self, email: &Email) -> Result<AccountId, ProvisionError> {
            let mut held = self.written.lock().expect("the lock");
            if held.accounts.iter().any(|(carried, _)| carried == email) {
                return Err(ProvisionError::Taken);
            }
            let account = AccountId::new(Uuid::new_v4());
            held.accounts.push((email.clone(), account));
            Ok(account)
        }

        async fn invite(
            &self,
            account: AccountId,
            _digest: SecretHash,
        ) -> Result<(), ProvisionError> {
            self.written.lock().expect("the lock").invited.push(account);
            Ok(())
        }

        async fn claim_project(
            &self,
            org: TenantId,
            name: &ProjectName,
        ) -> Result<ProjectId, ProvisionError> {
            let mut held = self.written.lock().expect("the lock");
            if held
                .projects
                .iter()
                .any(|(carried, called, _)| *carried == org && called == name)
            {
                return Err(ProvisionError::Taken);
            }
            let project = ProjectId::new(Uuid::new_v4());
            held.projects.push((org, name.clone(), project));
            Ok(project)
        }

        async fn only_project(&self, org: TenantId) -> Result<Option<ProjectId>, ProvisionError> {
            let held = self.written.lock().expect("the lock");
            let mut of_org = held
                .projects
                .iter()
                .filter(|(carried, _, _)| *carried == org)
                .map(|(_, _, project)| *project);
            match (of_org.next(), of_org.next()) {
                (Some(only), None) => Ok(Some(only)),
                _ => Ok(None),
            }
        }

        async fn issue_key(
            &self,
            org: TenantId,
            project: Option<ProjectId>,
        ) -> Result<IngestKey, ProvisionError> {
            self.written
                .lock()
                .expect("the lock")
                .issued
                .push((org, project));
            Ok(IngestKey::from_plaintext("ct_live_abc.the-secret"))
        }

        async fn revoke_key(&self, prefix: &str) -> Result<bool, ProvisionError> {
            Ok(prefix == "ct_live_abc")
        }
    }

    fn a_control_plane() -> (FakeControlPlane, Arc<Mutex<Written>>) {
        let written = Arc::new(Mutex::new(Written::default()));
        let fake = FakeControlPlane {
            written: Arc::clone(&written),
            unreachable: false,
        };
        (fake, written)
    }

    fn a_control_plane_holding(
        email: &Email,
        account: AccountId,
    ) -> (FakeControlPlane, Arc<Mutex<Written>>) {
        let (fake, written) = a_control_plane();
        written
            .lock()
            .expect("the lock")
            .accounts
            .push((email.clone(), account));
        (fake, written)
    }

    fn an_unreachable_control_plane() -> FakeControlPlane {
        let (fake, _) = a_control_plane();
        FakeControlPlane {
            unreachable: true,
            ..fake
        }
    }

    fn written(held: &Arc<Mutex<Written>>) -> Written {
        held.lock().expect("the lock").clone()
    }

    fn an_address() -> Email {
        Email::try_from("operator@example.com").expect("an address")
    }

    fn a_name() -> OrgName {
        OrgName::try_from("Default").expect("a name")
    }

    fn another_project() -> ProjectName {
        ProjectName::try_from("payments").expect("a slug")
    }

    fn founded(outcome: Bootstrapped) -> Founded {
        match outcome {
            Bootstrapped::Founded(founded) => founded,
            Bootstrapped::AlreadyFounded { existing } => {
                panic!("a fresh deployment answered with {existing}")
            }
        }
    }

    #[tokio::test]
    async fn a_bootstrap_founds_an_organisation_its_first_project_and_one_invitation() {
        let email = an_address();
        let (control_plane, held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);

        let outcome = provisioning
            .bootstrap(&a_name(), &email, true)
            .await
            .expect("a fresh deployment");

        let founded = founded(outcome);
        assert_eq!(founded.project_name.as_str(), "default");
        assert!(founded.invitation.is_some());
        let written = written(&held);
        assert_eq!(written.org, Some(founded.org));
        assert_eq!(written.org_name, Some(a_name()));
        assert_eq!(written.invited.len(), 1);
        assert_eq!(
            written.projects,
            vec![(founded.org, founded.project_name, founded.project)]
        );
    }

    #[tokio::test]
    async fn a_second_bootstrap_changes_nothing() {
        let email = an_address();
        let (control_plane, held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);
        provisioning
            .bootstrap(&a_name(), &email, true)
            .await
            .expect("a fresh deployment");
        let after_the_first = written(&held);

        let outcome = provisioning
            .bootstrap(&a_name(), &email, true)
            .await
            .expect("a deployment that already holds one");

        assert_eq!(
            outcome,
            Bootstrapped::AlreadyFounded {
                existing: after_the_first.org.expect("an organisation"),
            }
        );
        assert_eq!(written(&held), after_the_first);
    }

    #[tokio::test]
    async fn a_bootstrap_without_local_passwords_invites_nobody() {
        let email = an_address();
        let (control_plane, held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);

        let outcome = provisioning
            .bootstrap(&a_name(), &email, false)
            .await
            .expect("a fresh deployment");

        assert_eq!(founded(outcome).invitation, None);
        assert!(written(&held).invited.is_empty());
    }

    #[tokio::test]
    async fn a_bootstrap_keeps_the_account_that_already_carries_the_address() {
        let email = an_address();
        let account = AccountId::new(Uuid::from_u128(42));
        let (control_plane, held) = a_control_plane_holding(&email, account);
        let provisioning = Provisioning::new(control_plane);

        provisioning
            .bootstrap(&a_name(), &email, true)
            .await
            .expect("a fresh deployment");

        let written = written(&held);
        assert_eq!(written.accounts.len(), 1);
        assert_eq!(written.invited, vec![account]);
    }

    #[tokio::test]
    async fn a_key_takes_the_only_project_of_its_organisation() {
        let email = an_address();
        let (control_plane, held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);
        let founded = founded(
            provisioning
                .bootstrap(&a_name(), &email, true)
                .await
                .expect("a fresh deployment"),
        );

        let issued = provisioning
            .issue_key(founded.org, None)
            .await
            .expect("a key");

        assert_eq!(issued.project, Some(founded.project));
        assert_eq!(
            written(&held).issued,
            vec![(founded.org, Some(founded.project))]
        );
    }

    #[tokio::test]
    async fn a_key_names_no_project_when_the_organisation_holds_several() {
        let email = an_address();
        let (control_plane, _held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);
        let founded = founded(
            provisioning
                .bootstrap(&a_name(), &email, true)
                .await
                .expect("a fresh deployment"),
        );
        provisioning
            .create_project(founded.org, &another_project())
            .await
            .expect("a second project");

        let issued = provisioning
            .issue_key(founded.org, None)
            .await
            .expect("a key");

        assert_eq!(issued.project, None);
    }

    #[tokio::test]
    async fn a_key_keeps_the_project_it_was_given() {
        let (control_plane, _held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);
        let org = TenantId::new(Uuid::from_u128(7));
        let named = ProjectId::new(Uuid::from_u128(9));

        let issued = provisioning
            .issue_key(org, Some(named))
            .await
            .expect("a key");

        assert_eq!(issued.project, Some(named));
    }

    #[tokio::test]
    async fn a_project_name_the_organisation_already_holds_is_refused() {
        let email = an_address();
        let (control_plane, _held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);
        let founded = founded(
            provisioning
                .bootstrap(&a_name(), &email, true)
                .await
                .expect("a fresh deployment"),
        );

        let outcome = provisioning
            .create_project(founded.org, &founded.project_name)
            .await;

        assert_eq!(outcome.err(), Some(ProvisionError::Taken));
    }

    #[tokio::test]
    async fn a_reset_refuses_an_address_nobody_carries() {
        let (control_plane, _held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);

        let outcome = provisioning.reset_password(&an_address()).await;

        assert_eq!(outcome.err(), Some(ProvisionError::NoSuchAccount));
    }

    #[tokio::test]
    async fn a_reset_invites_the_account_that_carries_the_address() {
        let email = an_address();
        let account = AccountId::new(Uuid::from_u128(11));
        let (control_plane, held) = a_control_plane_holding(&email, account);
        let provisioning = Provisioning::new(control_plane);

        let token = provisioning
            .reset_password(&email)
            .await
            .expect("an invitation");

        assert!(token.looks_like_one());
        assert_eq!(written(&held).invited, vec![account]);
    }

    #[tokio::test]
    async fn a_new_account_is_refused_when_the_address_is_taken() {
        let email = an_address();
        let (control_plane, _held) =
            a_control_plane_holding(&email, AccountId::new(Uuid::from_u128(3)));
        let provisioning = Provisioning::new(control_plane);

        let outcome = provisioning.create_account(&email).await;

        assert_eq!(outcome.err(), Some(ProvisionError::Taken));
    }

    #[tokio::test]
    async fn a_new_account_is_created_locked_and_invited() {
        let (control_plane, held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);

        let token = provisioning
            .create_account(&an_address())
            .await
            .expect("an invitation");

        assert!(token.looks_like_one());
        let written = written(&held);
        assert_eq!(written.accounts.len(), 1);
        assert_eq!(written.invited.len(), 1);
    }

    #[tokio::test]
    async fn an_unreachable_control_plane_surfaces_to_the_caller() {
        let provisioning = Provisioning::new(an_unreachable_control_plane());

        let outcome = provisioning.bootstrap(&a_name(), &an_address(), true).await;

        assert_eq!(outcome.err(), Some(ProvisionError::Unreachable));
    }

    #[tokio::test]
    async fn a_revocation_says_whether_it_stopped_a_key() {
        let (control_plane, _held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);

        assert_eq!(provisioning.revoke_key("ct_live_abc").await, Ok(true));
        assert_eq!(provisioning.revoke_key("ct_live_zzz").await, Ok(false));
    }

    #[tokio::test]
    async fn a_role_is_granted_to_the_account_that_carries_the_address() {
        let email = an_address();
        let account = AccountId::new(Uuid::from_u128(5));
        let (control_plane, held) = a_control_plane_holding(&email, account);
        let provisioning = Provisioning::new(control_plane);
        let org = TenantId::new(Uuid::from_u128(8));

        provisioning
            .grant_role(org, &email, OrgRole::Admin)
            .await
            .expect("a granted role");

        assert_eq!(written(&held).granted, vec![(org, email, OrgRole::Admin)]);
    }

    #[tokio::test]
    async fn a_role_for_an_address_nobody_carries_is_refused() {
        let (control_plane, _held) = a_control_plane();
        let provisioning = Provisioning::new(control_plane);

        let outcome = provisioning
            .grant_role(
                TenantId::new(Uuid::from_u128(8)),
                &an_address(),
                OrgRole::Member,
            )
            .await;

        assert_eq!(outcome.err(), Some(ProvisionError::NoSuchAccount));
    }
}
