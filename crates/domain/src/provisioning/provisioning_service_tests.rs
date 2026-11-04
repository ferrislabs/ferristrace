use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::errors::{FoundError, ProvisionError};
use super::founding::Founding;
use super::ports::ControlPlane;
use super::provisioning_service::Provisioning;
use super::value_objects::{Bootstrapped, Founded, Minted, Revoked};
use crate::access::KeyId;
use crate::audit::{Action, Actor, Target};
use crate::catalogue::{ProjectId, ProjectName};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Email, Session, SessionError, SessionStore, SessionSwitch};
use crate::organisation::{OrgName, OrgRole};
use crate::secret::SessionToken;
use crate::telemetry::{IngestKey, TenantId};
use crate::testing::Notebook;

const ISSUED_KEY: u128 = 77;
const KEYS_ORG: u128 = 7;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Written {
    org: Option<TenantId>,
    org_name: Option<OrgName>,
    accounts: Vec<(Email, AccountId)>,
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

    async fn create_account(&self, email: &Email) -> Result<AccountId, ProvisionError> {
        let mut held = self.written.lock().expect("the lock");
        if held.accounts.iter().any(|(carried, _)| carried == email) {
            return Err(ProvisionError::Taken);
        }
        let account = AccountId::new(Uuid::new_v4());
        held.accounts.push((email.clone(), account));
        Ok(account)
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
    ) -> Result<Minted, ProvisionError> {
        self.written
            .lock()
            .expect("the lock")
            .issued
            .push((org, project));
        Ok(Minted {
            id: KeyId::new(Uuid::from_u128(ISSUED_KEY)),
            key: IngestKey::from_plaintext("ft_live_abc.the-secret"),
        })
    }

    async fn revoke_key(&self, prefix: &str) -> Result<Option<Revoked>, ProvisionError> {
        Ok((prefix == "ft_live_abc").then(|| Revoked {
            org: TenantId::new(Uuid::from_u128(KEYS_ORG)),
            id: KeyId::new(Uuid::from_u128(ISSUED_KEY)),
        }))
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
async fn a_bootstrap_founds_an_organisation_and_its_first_project() {
    let email = an_address();
    let (control_plane, held) = a_control_plane();
    let provisioning = Provisioning::new(control_plane, Notebook::default());

    let outcome = provisioning
        .bootstrap(&a_name(), &email)
        .await
        .expect("a fresh deployment");

    let founded = founded(outcome);
    assert_eq!(founded.project_name.as_str(), "default");
    let written = written(&held);
    assert_eq!(written.org, Some(founded.org));
    assert_eq!(written.org_name, Some(a_name()));
    assert_eq!(
        written.projects,
        vec![(founded.org, founded.project_name, founded.project)]
    );
}

#[tokio::test]
async fn a_second_bootstrap_changes_nothing() {
    let email = an_address();
    let (control_plane, held) = a_control_plane();
    let provisioning = Provisioning::new(control_plane, Notebook::default());
    provisioning
        .bootstrap(&a_name(), &email)
        .await
        .expect("a fresh deployment");
    let after_the_first = written(&held);

    let outcome = provisioning
        .bootstrap(&a_name(), &email)
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
async fn a_bootstrap_keeps_the_account_that_already_carries_the_address() {
    let email = an_address();
    let account = AccountId::new(Uuid::from_u128(42));
    let (control_plane, held) = a_control_plane_holding(&email, account);
    let provisioning = Provisioning::new(control_plane, Notebook::default());

    provisioning
        .bootstrap(&a_name(), &email)
        .await
        .expect("a fresh deployment");

    let written = written(&held);
    assert_eq!(written.accounts, vec![(email, account)]);
}

#[tokio::test]
async fn a_key_takes_the_only_project_of_its_organisation() {
    let email = an_address();
    let (control_plane, held) = a_control_plane();
    let provisioning = Provisioning::new(control_plane, Notebook::default());
    let founded = founded(
        provisioning
            .bootstrap(&a_name(), &email)
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
    let provisioning = Provisioning::new(control_plane, Notebook::default());
    let founded = founded(
        provisioning
            .bootstrap(&a_name(), &email)
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
    let provisioning = Provisioning::new(control_plane, Notebook::default());
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
    let provisioning = Provisioning::new(control_plane, Notebook::default());
    let founded = founded(
        provisioning
            .bootstrap(&a_name(), &email)
            .await
            .expect("a fresh deployment"),
    );

    let outcome = provisioning
        .create_project(founded.org, &founded.project_name)
        .await;

    assert_eq!(outcome.err(), Some(ProvisionError::Taken));
}

#[tokio::test]
async fn an_unreachable_control_plane_surfaces_to_the_caller() {
    let provisioning = Provisioning::new(an_unreachable_control_plane(), Notebook::default());

    let outcome = provisioning.bootstrap(&a_name(), &an_address()).await;

    assert_eq!(outcome.err(), Some(ProvisionError::Unreachable));
}

#[tokio::test]
async fn a_revocation_says_whether_it_stopped_a_key() {
    let (control_plane, _held) = a_control_plane();
    let provisioning = Provisioning::new(control_plane, Notebook::default());

    assert_eq!(provisioning.revoke_key("ft_live_abc").await, Ok(true));
    assert_eq!(provisioning.revoke_key("ft_live_zzz").await, Ok(false));
}

#[tokio::test]
async fn a_role_is_granted_to_the_account_that_carries_the_address() {
    let email = an_address();
    let account = AccountId::new(Uuid::from_u128(5));
    let (control_plane, held) = a_control_plane_holding(&email, account);
    let provisioning = Provisioning::new(control_plane, Notebook::default());
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
    let provisioning = Provisioning::new(control_plane, Notebook::default());

    let outcome = provisioning
        .grant_role(
            TenantId::new(Uuid::from_u128(8)),
            &an_address(),
            OrgRole::Member,
        )
        .await;

    assert_eq!(outcome.err(), Some(ProvisionError::NoSuchAccount));
}

#[tokio::test]
async fn a_bootstrap_records_one_founding_by_the_operator_and_a_second_records_none() {
    let email = an_address();
    let (control_plane, _held) = a_control_plane();
    let notebook = Notebook::default();
    let provisioning = Provisioning::new(control_plane, notebook.clone());

    let founded = founded(
        provisioning
            .bootstrap(&a_name(), &email)
            .await
            .expect("a fresh deployment"),
    );
    provisioning
        .bootstrap(&a_name(), &email)
        .await
        .expect("a deployment that already holds one");

    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].org, founded.org);
    assert_eq!(entries[0].actor, Actor::Operator);
    assert_eq!(entries[0].action, Action::OrgFounded);
    assert_eq!(entries[0].target, Target::org(founded.org));
    assert_eq!(entries[0].detail.get("name"), Some(Some("Default")));
    assert_eq!(
        entries[0].detail.get("owner"),
        Some(Some("operator@example.com"))
    );
}

#[tokio::test]
async fn an_unreachable_bootstrap_records_nothing() {
    let notebook = Notebook::default();
    let provisioning = Provisioning::new(an_unreachable_control_plane(), notebook.clone());

    let outcome = provisioning.bootstrap(&a_name(), &an_address()).await;

    assert_eq!(outcome.err(), Some(ProvisionError::Unreachable));
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn each_command_line_change_is_recorded_as_the_operator() {
    let email = an_address();
    let account = AccountId::new(Uuid::from_u128(5));
    let (control_plane, _held) = a_control_plane_holding(&email, account);
    let notebook = Notebook::default();
    let provisioning = Provisioning::new(control_plane, notebook.clone());

    let org = provisioning
        .create_org(&a_name(), &email)
        .await
        .expect("a new organisation");
    provisioning
        .grant_role(org, &email, OrgRole::Admin)
        .await
        .expect("a granted role");
    let project = provisioning
        .create_project(org, &another_project())
        .await
        .expect("a project");
    provisioning
        .issue_key(org, Some(project))
        .await
        .expect("a key");
    provisioning
        .revoke_key("ft_live_abc")
        .await
        .expect("a revocation");
    provisioning
        .revoke_key("ft_live_zzz")
        .await
        .expect("nothing to revoke");

    let entries = notebook.entries();
    let actions: Vec<Action> = entries.iter().map(|entry| entry.action).collect();
    assert_eq!(
        actions,
        vec![
            Action::OrgFounded,
            Action::MemberRoleGranted,
            Action::ProjectCreated,
            Action::KeyIssued,
            Action::KeyRevoked,
        ]
    );
    assert!(entries.iter().all(|entry| entry.actor == Actor::Operator));
    assert_eq!(entries[1].target, Target::member(account));
    assert_eq!(entries[1].detail.get("to"), Some(Some("admin")));
    assert_eq!(entries[2].target, Target::project(project));
    assert_eq!(entries[2].detail.get("name"), Some(Some("payments")));
    let key = KeyId::new(Uuid::from_u128(ISSUED_KEY));
    assert_eq!(entries[3].target, Target::key(key));
    assert_eq!(entries[3].detail.get("scopes"), Some(Some("ingest")));
    assert_eq!(entries[4].org, TenantId::new(Uuid::from_u128(KEYS_ORG)));
    assert_eq!(entries[4].target, Target::key(key));
    assert!(
        entries
            .iter()
            .all(|entry| entry.detail.pairs().iter().all(|(_, value)| {
                value
                    .as_deref()
                    .is_none_or(|value| !value.contains("the-secret"))
            }))
    );
}

const LIVE_TOKEN: &str = "ft_ss_live";

#[derive(Debug, Clone)]
struct OneSession {
    account: AccountId,
    email: Email,
    switched: Arc<Mutex<Vec<TenantId>>>,
    unreachable: bool,
}

impl OneSession {
    fn of(account: AccountId, email: &Email) -> Self {
        Self {
            account,
            email: email.clone(),
            switched: Arc::default(),
            unreachable: false,
        }
    }
}

impl SessionStore for OneSession {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() != LIVE_TOKEN {
            return Err(SessionError::Unknown);
        }
        Ok(Session {
            account: self.account,
            email: self.email.clone(),
            name: None,
            photo: None,
            org: None,
            expires_at: UnixSeconds::new(0),
        })
    }
}

impl SessionSwitch for OneSession {
    async fn switch(&self, _token: &SessionToken, org: TenantId) -> Result<(), SessionError> {
        if self.unreachable {
            return Err(SessionError::StoreUnreachable);
        }
        self.switched.lock().expect("the lock").push(org);
        Ok(())
    }
}

fn a_live_token() -> SessionToken {
    SessionToken::for_subject(LIVE_TOKEN)
}

#[tokio::test]
async fn a_signed_in_account_founds_an_organisation_it_owns_with_a_default_project() {
    let email = an_address();
    let account = AccountId::new(Uuid::from_u128(11));
    let (control_plane, held) = a_control_plane_holding(&email, account);
    let sessions = OneSession::of(account, &email);
    let notebook = Notebook::default();
    let founding = Founding::new(sessions.clone(), control_plane, notebook.clone());

    let founded = founding
        .found(&a_live_token(), &a_name())
        .await
        .expect("a founded organisation");

    let written = written(&held);
    assert_eq!(written.org, Some(founded.org));
    assert_eq!(written.org_name, Some(a_name()));
    assert_eq!(founded.project_name.as_str(), "default");
    assert_eq!(
        written.projects,
        vec![(founded.org, founded.project_name, founded.project)]
    );
    assert_eq!(
        *sessions.switched.lock().expect("the lock"),
        vec![founded.org]
    );
}

#[tokio::test]
async fn a_web_founding_is_recorded_once_under_the_account_that_made_it() {
    let email = an_address();
    let account = AccountId::new(Uuid::from_u128(11));
    let (control_plane, _held) = a_control_plane_holding(&email, account);
    let notebook = Notebook::default();
    let founding = Founding::new(
        OneSession::of(account, &email),
        control_plane,
        notebook.clone(),
    );

    let founded = founding
        .found(&a_live_token(), &a_name())
        .await
        .expect("a founded organisation");

    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].org, founded.org);
    assert_eq!(entries[0].actor, Actor::Account(account));
    assert_eq!(entries[0].action, Action::OrgFounded);
    assert_eq!(entries[0].target, Target::org(founded.org));
    assert_eq!(entries[0].detail.get("owner"), Some(Some(email.as_str())));
}

#[tokio::test]
async fn a_token_that_resolves_to_nothing_founds_nothing() {
    let email = an_address();
    let account = AccountId::new(Uuid::from_u128(11));
    let (control_plane, held) = a_control_plane_holding(&email, account);
    let notebook = Notebook::default();
    let founding = Founding::new(
        OneSession::of(account, &email),
        control_plane,
        notebook.clone(),
    );

    let refused = founding
        .found(&SessionToken::for_subject("ft_ss_other"), &a_name())
        .await;

    assert_eq!(refused, Err(FoundError::NoSession));
    assert_eq!(written(&held).org, None);
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn a_session_that_cannot_be_switched_refuses_the_founding() {
    let email = an_address();
    let account = AccountId::new(Uuid::from_u128(11));
    let (control_plane, _held) = a_control_plane_holding(&email, account);
    let sessions = OneSession {
        unreachable: true,
        ..OneSession::of(account, &email)
    };
    let founding = Founding::new(sessions, control_plane, Notebook::default());

    let refused = founding.found(&a_live_token(), &a_name()).await;

    assert_eq!(refused, Err(FoundError::Unreachable));
}

#[tokio::test]
async fn an_account_the_control_plane_does_not_know_founds_nothing() {
    let email = an_address();
    let account = AccountId::new(Uuid::from_u128(11));
    let (control_plane, held) = a_control_plane();
    let notebook = Notebook::default();
    let founding = Founding::new(
        OneSession::of(account, &email),
        control_plane,
        notebook.clone(),
    );

    let refused = founding.found(&a_live_token(), &a_name()).await;

    assert_eq!(refused, Err(FoundError::NoSession));
    assert_eq!(written(&held).org, None);
    assert!(notebook.entries().is_empty());
}
