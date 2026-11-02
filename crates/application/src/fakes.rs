use std::sync::atomic::Ordering;

use ferristrace_domain::{
    AccountId, Agent, ApiKey, ApiKeys, ApiSecret, Assigned, AuditEntry, AuditFilter, AuditPage,
    AuditTrail, ControlPlane, Created, Email, ExportSource, Fingerprint, IssueDecisions, IssuedKey,
    KeyId, KeyName, KeyScopes, LiveSession, Membership, Memberships, Minted, NoPhotos, OrgName,
    OrgRole, Organisation, Orgs, PageSize, Portrait, Project, ProjectId, ProjectName, Projects,
    ProvisionError, ProvisionPolicy, ReadGrant, Recorded, Renamed, RetentionDays, Revoked,
    SecretHash, Session, SessionError, SessionId, SessionRoll, SessionStore, SessionToken,
    Snapshot, Status, StoreError, TeamId, TenantId, UnixSeconds,
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
    type LogStore = Inert;
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
pub struct FakeMemberships {
    handle: Handle,
}

impl FakeMemberships {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        tx.note_build();
        Self { handle: tx.clone() }
    }
}

impl Memberships for FakeMemberships {
    async fn of(
        &self,
        _account: AccountId,
    ) -> Result<Vec<Membership>, ferristrace_domain::WhoamiError> {
        let role = if self.handle.ledger.member_only.load(Ordering::SeqCst) {
            OrgRole::Member
        } else {
            OrgRole::Owner
        };
        Ok(vec![Membership {
            org: the_org(),
            name: "Acme".into(),
            role,
        }])
    }
}

#[repository(domain = Audit, backend = Recording)]
#[derive(Debug)]
pub struct FakeAudit {
    handle: Handle,
}

impl FakeAudit {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        Self { handle: tx.clone() }
    }
}

impl AuditTrail for FakeAudit {
    async fn record(&self, entry: &AuditEntry) -> Result<(), StoreError> {
        self.handle.note(entry);
        Ok(())
    }

    async fn list(
        &self,
        _org: TenantId,
        _filter: &AuditFilter,
        _page: PageSize,
    ) -> Result<AuditPage, StoreError> {
        Ok(AuditPage::default())
    }
}

#[repository(domain = Export, backend = Recording)]
#[derive(Debug)]
pub struct FakeExport;

impl FakeExport {
    fn new(_backend: &Recording, _tx: &Handle) -> Self {
        Self
    }
}

impl ExportSource for FakeExport {
    async fn snapshot(&self, org: TenantId) -> Result<Snapshot, StoreError> {
        Ok(Snapshot {
            org: Organisation {
                id: org,
                name: "Acme".into(),
                plan: "free".into(),
                created_at_unix_nanos: 0,
                logo: None,
                retention: None,
                retention_cap: None,
            },
            projects: Vec::new(),
            teams: Vec::new(),
            members: Vec::new(),
            keys: Vec::new(),
            decisions: Vec::new(),
            audit: Vec::new(),
        })
    }
}

#[repository(domain = Decisions, backend = Recording)]
#[derive(Debug)]
pub struct FakeDecisions {
    handle: Handle,
}

impl FakeDecisions {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        Self { handle: tx.clone() }
    }
}

impl IssueDecisions for FakeDecisions {
    async fn read_many(
        &self,
        _of: &[(ProjectId, Fingerprint)],
    ) -> Result<Vec<Recorded>, StoreError> {
        Ok(Vec::new())
    }

    async fn decide(
        &self,
        _project: ProjectId,
        _print: Fingerprint,
        _status: Status,
        _by: AccountId,
        _at_unix_nanos: u64,
    ) -> Result<(), StoreError> {
        self.handle.write();
        Ok(())
    }

    async fn assign(
        &self,
        _project: ProjectId,
        _print: Fingerprint,
        _owner: Option<AccountId>,
        _by: AccountId,
        _at_unix_nanos: u64,
    ) -> Result<(), StoreError> {
        if self.handle.ledger.refuse_assignments.load(Ordering::SeqCst) {
            return Err(StoreError::Unreachable);
        }
        self.handle.write();
        Ok(())
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
    async fn only(&self, org: TenantId) -> Result<Option<Organisation>, StoreError> {
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
            retention: None,
            retention_cap: None,
        }))
    }

    async fn rename(&self, _org: TenantId, name: &OrgName) -> Result<(), StoreError> {
        self.handle.write();
        *self.handle.ledger.org_name.lock().expect("the ledger lock") = name.as_str().to_owned();
        Ok(())
    }

    async fn set_logo(&self, _org: TenantId, _logo: Option<&Portrait>) -> Result<(), StoreError> {
        Ok(())
    }

    async fn set_retention(
        &self,
        _org: TenantId,
        _retention: Option<RetentionDays>,
    ) -> Result<(), StoreError> {
        self.handle.write();
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
    ) -> Result<Minted, ProvisionError> {
        Err(ProvisionError::Unreachable)
    }

    async fn revoke_key(&self, _prefix: &str) -> Result<Option<Revoked>, ProvisionError> {
        Ok(None)
    }

    async fn memberships_of(&self, _account: AccountId) -> Result<Vec<TenantId>, ProvisionError> {
        Ok(Vec::new())
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

#[repository(domain = ApiKeys, backend = Recording)]
#[derive(Debug)]
pub struct FakeApiKeys {
    handle: Handle,
}

impl FakeApiKeys {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        tx.note_build();
        Self { handle: tx.clone() }
    }
}

impl ApiKeys for FakeApiKeys {
    async fn list(&self, _org: TenantId) -> Result<Vec<ApiKey>, StoreError> {
        Ok(Vec::new())
    }

    async fn issue(
        &self,
        _org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        by: AccountId,
    ) -> Result<IssuedKey, StoreError> {
        self.handle.write();
        Ok(IssuedKey {
            key: ApiKey {
                id: KeyId::new(Uuid::from_u128(4)),
                project,
                name: name.as_str().into(),
                prefix: "ft_live_000000000004".into(),
                scopes: scopes.clone(),
                created_at_unix_nanos: 0,
                created_by: Some(by),
                last_used_at_unix_nanos: None,
            },
            secret: ApiSecret::from_plaintext("ft_live_000000000004.secret"),
        })
    }

    async fn revoke(&self, _org: TenantId, _id: KeyId) -> Result<bool, StoreError> {
        self.handle.attempt();
        Ok(false)
    }

    async fn grant_for(&self, _secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(None)
    }
}

#[repository(domain = Projects, backend = Recording)]
#[derive(Debug)]
pub struct FakeProjects;

impl FakeProjects {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        tx.note_build();
        Self
    }
}

impl Projects for FakeProjects {
    async fn of(&self, _tenant: TenantId) -> Result<Vec<Project>, StoreError> {
        Ok(Vec::new())
    }

    async fn create(&self, _org: TenantId, _name: &ProjectName) -> Result<Created, StoreError> {
        Ok(Created::Done)
    }

    async fn rename(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _name: &ProjectName,
    ) -> Result<Renamed, StoreError> {
        Ok(Renamed::Gone)
    }

    async fn forget(&self, _org: TenantId, _project: ProjectId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn assign(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _team: Option<TeamId>,
    ) -> Result<Assigned, StoreError> {
        Ok(Assigned::Done)
    }
}
