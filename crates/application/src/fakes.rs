use std::sync::atomic::Ordering;

use ferristrace_domain::{
    AccountId, Agent, ControlPlane, Email, IngestKey, LiveSession, Membership, Memberships,
    NoPhotos, OrgName, OrgRole, Organisation, Orgs, Portrait, ProjectId, ProjectName,
    ProvisionError, ProvisionPolicy, ReadError, SecretHash, Session, SessionError, SessionId,
    SessionRoll, SessionStore, SessionToken, TenantId, UnixSeconds,
};
use uuid::Uuid;

use crate::recording::{Handle, Recording};
use crate::testing::Inert;
use crate::{Stack, repository};

pub fn the_org() -> TenantId {
    TenantId::new(Uuid::from_u128(1))
}

pub fn the_account() -> AccountId {
    AccountId::new(Uuid::from_u128(2))
}

pub fn the_email() -> Email {
    Email::try_from("owner@example.com").expect("an address")
}

#[derive(Debug)]
pub struct Probing;

impl Stack for Probing {
    type Repos = Recording;
    type Clock = Inert;
    type Hasher = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = Inert;
    type Meter = Inert;
    type Oidc = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
}

#[repository(domain = Sessions, backend = Recording)]
#[derive(Debug)]
pub struct FakeSessions {
    handle: Handle,
}

impl FakeSessions {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        tx.note_build();
        Self { handle: tx.clone() }
    }
}

impl SessionStore for FakeSessions {
    async fn issue(
        &self,
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Err(SessionError::Unknown)
    }

    async fn resolve(&self, _token: &SessionToken) -> Result<Session, SessionError> {
        self.handle.attempt();
        Ok(Session {
            account: the_account(),
            email: the_email(),
            name: None,
            photo: None,
            org: None,
            expires_at: UnixSeconds::new(1),
        })
    }

    async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
        Ok(())
    }
}

impl SessionRoll for FakeSessions {
    async fn live(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<Vec<LiveSession>, SessionError> {
        Ok(Vec::new())
    }

    async fn revoke_one(&self, _account: AccountId, _id: SessionId) -> Result<bool, SessionError> {
        Ok(false)
    }

    async fn revoke_others(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<u64, SessionError> {
        Ok(0)
    }
}

#[repository(domain = Memberships, backend = Recording)]
#[derive(Debug)]
pub struct FakeMemberships;

impl FakeMemberships {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        tx.note_build();
        Self
    }
}

impl Memberships for FakeMemberships {
    async fn of(
        &self,
        _account: AccountId,
    ) -> Result<Vec<Membership>, ferristrace_domain::WhoamiError> {
        Ok(vec![Membership {
            org: the_org(),
            name: "Acme".into(),
            role: OrgRole::Owner,
        }])
    }
}

#[repository(domain = Orgs, backend = Recording)]
#[derive(Debug)]
pub struct FakeOrgs {
    handle: Handle,
}

impl FakeOrgs {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        tx.note_build();
        Self { handle: tx.clone() }
    }
}

impl Orgs for FakeOrgs {
    async fn only(&self, org: TenantId) -> Result<Option<Organisation>, ReadError> {
        let name = self
            .handle
            .ledger
            .org_name
            .lock()
            .expect("the ledger lock")
            .clone();
        Ok(Some(Organisation {
            id: org,
            name: name.into(),
            plan: "free".into(),
            created_at_unix_nanos: 0,
            logo: None,
        }))
    }

    async fn rename(&self, _org: TenantId, name: &OrgName) -> Result<(), ReadError> {
        self.handle.write();
        *self.handle.ledger.org_name.lock().expect("the ledger lock") = name.as_str().to_owned();
        Ok(())
    }

    async fn set_logo(&self, _org: TenantId, _logo: Option<&Portrait>) -> Result<(), ReadError> {
        Ok(())
    }
}

#[repository(domain = ControlPlane, backend = Recording)]
#[derive(Debug)]
pub struct FakeControlPlane {
    handle: Handle,
}

impl FakeControlPlane {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        tx.note_build();
        Self { handle: tx.clone() }
    }
}

impl ControlPlane for FakeControlPlane {
    async fn founded(&self) -> Result<Option<TenantId>, ProvisionError> {
        self.handle.attempt();
        Ok(None)
    }

    async fn found(&self, _name: &OrgName, _owner: &Email) -> Result<TenantId, ProvisionError> {
        self.handle.write();
        Ok(the_org())
    }

    async fn grant(
        &self,
        _org: TenantId,
        _member: &Email,
        _role: OrgRole,
    ) -> Result<(), ProvisionError> {
        self.handle.write();
        Ok(())
    }

    async fn account_for(&self, _email: &Email) -> Result<Option<AccountId>, ProvisionError> {
        self.handle.attempt();
        Ok(Some(the_account()))
    }

    async fn create_locked_account(&self, _email: &Email) -> Result<AccountId, ProvisionError> {
        self.handle.write();
        Ok(the_account())
    }

    async fn invite(&self, _account: AccountId, _digest: SecretHash) -> Result<(), ProvisionError> {
        self.handle.write();
        Ok(())
    }

    async fn claim_project(
        &self,
        _org: TenantId,
        _name: &ProjectName,
    ) -> Result<ProjectId, ProvisionError> {
        self.handle.attempt();
        if self.handle.ledger.refuse_claims.load(Ordering::SeqCst) {
            return Err(ProvisionError::Unreachable);
        }
        self.handle.write();
        Ok(ProjectId::new(Uuid::from_u128(3)))
    }

    async fn only_project(&self, _org: TenantId) -> Result<Option<ProjectId>, ProvisionError> {
        Ok(None)
    }

    async fn issue_key(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
    ) -> Result<IngestKey, ProvisionError> {
        Err(ProvisionError::Unreachable)
    }

    async fn revoke_key(&self, _prefix: &str) -> Result<bool, ProvisionError> {
        Ok(false)
    }
}

pub fn the_policy() -> ProvisionPolicy {
    ProvisionPolicy::default()
}

pub fn a_token() -> SessionToken {
    SessionToken::from_plaintext("a-token")
}

pub fn a_name(of: &str) -> OrgName {
    OrgName::try_from(of).expect("a name")
}
