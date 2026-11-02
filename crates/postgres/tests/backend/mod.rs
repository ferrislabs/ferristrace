#![allow(dead_code, unused_macros, unused_imports)]

use core::marker::PhantomData;

use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_domain::*;
use ferristrace_postgres::{AccountError, MembershipError, Postgres, ProjectError};

pub fn url() -> Option<String> {
    if let Ok(url) = std::env::var("DATABASE_URL") {
        return Some(url);
    }
    assert!(
        std::env::var("REQUIRE_DATABASE_URL").is_err(),
        "REQUIRE_DATABASE_URL is set but DATABASE_URL is not"
    );
    None
}

macro_rules! committed {
    ($backend:expr, |$($repo:ident: $domain:ident),+| $body:expr) => {{
        let backend = &$backend;
        ::ferristrace_application::registry::UnitOfWork::transaction(
            backend,
            || (),
            ::ferristrace_application::registry::Settle::OnSuccess,
            |tx| {
                let built = (
                    $(
                        <_ as ::ferristrace_application::registry::RepoFor<
                            ::ferristrace_application::registry::domain::$domain,
                        >>::build(backend, &tx),
                    )+
                );
                async {
                    let ($($repo,)+) = built;
                    Ok::<_, ()>($body)
                }
            },
        )
        .await
        .expect("the transaction commits")
    }};
}

pub struct Each<'b, D, C> {
    backend: &'b Postgres<C>,
    marker: PhantomData<fn() -> D>,
}

impl<'b, D, C> Each<'b, D, C> {
    pub fn over(backend: &'b Postgres<C>) -> Self {
        Self {
            backend,
            marker: PhantomData,
        }
    }
}

impl<D, C> Clone for Each<'_, D, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D, C> Copy for Each<'_, D, C> {}

pub fn sessions<C>(backend: &Postgres<C>) -> Each<'_, domain::Sessions, C> {
    Each::over(backend)
}

pub fn memberships<C>(backend: &Postgres<C>) -> Each<'_, domain::Memberships, C> {
    Each::over(backend)
}

pub fn accounts<C>(backend: &Postgres<C>) -> Each<'_, domain::Accounts, C> {
    Each::over(backend)
}

pub fn orgs<C>(backend: &Postgres<C>) -> Each<'_, domain::Orgs, C> {
    Each::over(backend)
}

pub fn projects<C>(backend: &Postgres<C>) -> Each<'_, domain::Projects, C> {
    Each::over(backend)
}

pub fn roster<C>(backend: &Postgres<C>) -> Each<'_, domain::Roster, C> {
    Each::over(backend)
}

pub fn teams<C>(backend: &Postgres<C>) -> Each<'_, domain::Teams, C> {
    Each::over(backend)
}

pub fn usage<C>(backend: &Postgres<C>) -> Each<'_, domain::UsageHistory, C> {
    Each::over(backend)
}

pub fn decisions<C>(backend: &Postgres<C>) -> Each<'_, domain::Decisions, C> {
    Each::over(backend)
}

pub fn factors<C>(backend: &Postgres<C>) -> Each<'_, domain::Factors, C> {
    Each::over(backend)
}

pub fn resets<C>(backend: &Postgres<C>) -> Each<'_, domain::Resets, C> {
    Each::over(backend)
}

pub fn flows<C>(backend: &Postgres<C>) -> Each<'_, domain::Flows, C> {
    Each::over(backend)
}

pub fn local<C>(backend: &Postgres<C>) -> Each<'_, domain::LocalPasswords, C> {
    Each::over(backend)
}

pub fn api_keys<C>(backend: &Postgres<C>) -> Each<'_, domain::ApiKeys, C> {
    Each::over(backend)
}

pub fn control_plane_of<C>(backend: &Postgres<C>) -> Each<'_, domain::ControlPlane, C> {
    Each::over(backend)
}

macro_rules! forward {
    ($domain:ident: $port:path { $( fn $name:ident(&self $(, $arg:ident: $ty:ty)*) -> $ret:ty; )+ }) => {
        impl<C> $port for Each<'_, domain::$domain, C>
        where
            C: Send + Sync + 'static,
            Postgres<C>: RepoFor<domain::$domain, Repo: $port>,
        {
            $(
                async fn $name(&self $(, $arg: $ty)*) -> $ret {
                    committed!((*self.backend), |repo: $domain| repo.$name($($arg),*).await)
                }
            )+
        }
    };
}

forward!(Sessions: SessionStore {
    fn issue(&self, account: AccountId, org: Option<TenantId>, from: Option<&Agent>) -> Result<SessionToken, SessionError>;
    fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError>;
    fn revoke(&self, token: &SessionToken) -> Result<(), SessionError>;
});

forward!(Sessions: SessionRoll {
    fn live(&self, account: AccountId, current: &SessionToken) -> Result<Vec<LiveSession>, SessionError>;
    fn revoke_one(&self, account: AccountId, id: SessionId) -> Result<bool, SessionError>;
    fn revoke_others(&self, account: AccountId, current: &SessionToken) -> Result<u64, SessionError>;
});

forward!(Memberships: Memberships {
    fn of(&self, account: AccountId) -> Result<Vec<Membership>, WhoamiError>;
});

forward!(Accounts: Accounts {
    fn rename(&self, account: AccountId, name: Option<&AccountName>) -> Result<(), StoreError>;
    fn portrait_of(&self, account: AccountId) -> Result<Option<Portrait>, StoreError>;
    fn set_portrait(&self, account: AccountId, portrait: Option<&Portrait>) -> Result<(), StoreError>;
});

forward!(Accounts: AccountDirectory {
    fn provision(&self, identity: &ResolvedIdentity, policy: ProvisionPolicy) -> Result<AccountId, AuthError>;
});

forward!(Orgs: Orgs {
    fn only(&self, org: TenantId) -> Result<Option<Organisation>, StoreError>;
    fn rename(&self, org: TenantId, name: &OrgName) -> Result<(), StoreError>;
    fn set_logo(&self, org: TenantId, logo: Option<&Portrait>) -> Result<(), StoreError>;
});

forward!(Projects: Projects {
    fn of(&self, tenant: TenantId) -> Result<Vec<Project>, StoreError>;
    fn create(&self, org: TenantId, name: &ProjectName) -> Result<Created, StoreError>;
    fn rename(&self, org: TenantId, project: ProjectId, name: &ProjectName) -> Result<Renamed, StoreError>;
    fn forget(&self, org: TenantId, project: ProjectId) -> Result<bool, StoreError>;
    fn assign(&self, org: TenantId, project: ProjectId, team: Option<TeamId>) -> Result<Assigned, StoreError>;
});

forward!(ApiKeys: ApiKeys {
    fn list(&self, org: TenantId) -> Result<Vec<ApiKey>, StoreError>;
    fn issue(&self, org: TenantId, project: Option<ProjectId>, name: &KeyName, scopes: &KeyScopes, by: AccountId) -> Result<IssuedKey, StoreError>;
    fn revoke(&self, org: TenantId, id: KeyId) -> Result<bool, StoreError>;
    fn grant_for(&self, secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError>;
});

forward!(Roster: Roll {
    fn of(&self, org: TenantId) -> Result<Vec<Member>, StoreError>;
    fn grant(&self, org: TenantId, account: AccountId, role: OrgRole) -> Result<Option<Member>, StoreError>;
    fn remove(&self, org: TenantId, account: AccountId) -> Result<bool, StoreError>;
    fn standing_of(&self, email: &Email) -> Result<Option<Holder>, StoreError>;
    fn create_locked(&self, email: &Email) -> Result<AccountId, StoreError>;
    fn join(&self, org: TenantId, account: AccountId, role: OrgRole) -> Result<Option<Joined>, StoreError>;
    fn open_way_in(&self, account: AccountId, digest: SecretHash) -> Result<(), StoreError>;
});

forward!(Teams: Squads {
    fn of(&self, org: TenantId) -> Result<Vec<Team>, StoreError>;
    fn create(&self, org: TenantId, name: &TeamName) -> Result<Named, StoreError>;
    fn rename(&self, org: TenantId, team: TeamId, name: &TeamName) -> Result<Named, StoreError>;
    fn disband(&self, org: TenantId, team: TeamId) -> Result<bool, StoreError>;
    fn seat(&self, org: TenantId, team: TeamId, members: &[AccountId]) -> Result<bool, StoreError>;
    fn own(&self, org: TenantId, team: TeamId, projects: &[ProjectId]) -> Result<bool, StoreError>;
});

forward!(UsageHistory: UsageHistory {
    fn hours(&self, org: TenantId, window: Window) -> Result<Vec<Consumed>, StoreError>;
    fn included(&self, org: TenantId) -> Result<Option<DecompressedBytes>, StoreError>;
});

forward!(Decisions: IssueDecisions {
    fn read_many(&self, of: &[(ProjectId, Fingerprint)]) -> Result<Vec<Recorded>, StoreError>;
    fn decide(&self, project: ProjectId, print: Fingerprint, status: Status, by: AccountId, at_unix_nanos: u64) -> Result<(), StoreError>;
    fn assign(&self, project: ProjectId, print: Fingerprint, owner: Option<AccountId>, by: AccountId, at_unix_nanos: u64) -> Result<(), StoreError>;
});

forward!(Factors: FactorStore {
    fn live(&self, account: AccountId) -> Result<Vec<FactorKind>, FactorError>;
    fn keep(&self, account: AccountId, kind: FactorKind, secret: &TotpSecret) -> Result<(), FactorError>;
    fn kept(&self, account: AccountId, kind: FactorKind) -> Result<Option<Kept>, FactorError>;
    fn confirm(&self, account: AccountId, kind: FactorKind) -> Result<(), FactorError>;
    fn forget(&self, account: AccountId, kind: FactorKind) -> Result<bool, FactorError>;
    fn spent_step(&self, account: AccountId, kind: FactorKind, step: Step) -> Result<(), FactorError>;
    fn keep_recovery(&self, account: AccountId, digests: &[SecretHash]) -> Result<(), FactorError>;
    fn spend_recovery(&self, account: AccountId, digest: SecretHash) -> Result<bool, FactorError>;
    fn recovery_left(&self, account: AccountId) -> Result<u32, FactorError>;
    fn answered(&self, digest: SecretHash) -> Result<Pending, FactorError>;
    fn close(&self, digest: SecretHash) -> Result<(), FactorError>;
});

forward!(Factors: Gate {
    fn offered(&self, account: AccountId) -> Result<Vec<FactorKind>, FactorError>;
    fn open(&self, account: AccountId, digest: SecretHash) -> Result<(), FactorError>;
});

forward!(Resets: ResetStore {
    fn account_for(&self, email: &Email) -> Result<Option<AccountId>, ResetError>;
    fn begin(&self, account: AccountId, digest: SecretHash) -> Result<(), ResetError>;
    fn take(&self, digest: SecretHash) -> Result<AccountId, ResetError>;
    fn set_password(&self, account: AccountId, hash: &PasswordHash) -> Result<(), ResetError>;
    fn revoke_sessions(&self, account: AccountId) -> Result<u64, ResetError>;
});

forward!(Flows: OidcFlows {
    fn begin(&self, flow: &PendingFlow) -> Result<(), FlowError>;
    fn take(&self, state: &str) -> Result<PendingFlow, FlowError>;
});

forward!(LocalPasswords: AuthProvider {
    fn sign_in(&self, proof: SignInProof) -> Result<ResolvedIdentity, AuthError>;
});

forward!(ControlPlane: ControlPlane {
    fn founded(&self) -> Result<Option<TenantId>, ProvisionError>;
    fn found(&self, name: &OrgName, owner: &Email) -> Result<TenantId, ProvisionError>;
    fn grant(&self, org: TenantId, member: &Email, role: OrgRole) -> Result<(), ProvisionError>;
    fn account_for(&self, email: &Email) -> Result<Option<AccountId>, ProvisionError>;
    fn create_locked_account(&self, email: &Email) -> Result<AccountId, ProvisionError>;
    fn invite(&self, account: AccountId, digest: SecretHash) -> Result<(), ProvisionError>;
    fn claim_project(&self, org: TenantId, name: &ProjectName) -> Result<ProjectId, ProvisionError>;
    fn only_project(&self, org: TenantId) -> Result<Option<ProjectId>, ProvisionError>;
    fn issue_key(&self, org: TenantId, project: Option<ProjectId>) -> Result<IngestKey, ProvisionError>;
    fn revoke_key(&self, prefix: &str) -> Result<bool, ProvisionError>;
});

impl<C: Send + Sync + 'static> Each<'_, domain::Memberships, C> {
    pub async fn founded(&self) -> Result<Option<TenantId>, MembershipError> {
        committed!((*self.backend), |repo: Memberships| repo.founded().await)
    }

    pub async fn found(&self, name: &str, owner: &str) -> Result<TenantId, MembershipError> {
        committed!((*self.backend), |repo: Memberships| repo
            .found(name, owner)
            .await)
    }

    pub async fn grant(
        &self,
        org: TenantId,
        member: &str,
        role: OrgRole,
    ) -> Result<(), MembershipError> {
        committed!((*self.backend), |repo: Memberships| repo
            .grant(org, member, role)
            .await)
    }
}

impl<C: Send + Sync + 'static> Each<'_, domain::Projects, C> {
    pub async fn claim(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<ProjectId, ProjectError> {
        committed!((*self.backend), |repo: Projects| repo
            .claim(org, name)
            .await)
    }

    pub async fn only(&self, org: TenantId) -> Result<Option<ProjectId>, ProjectError> {
        committed!((*self.backend), |repo: Projects| repo.only(org).await)
    }
}

impl<C: Clock + Clone + Send + Sync + 'static> Each<'_, domain::Factors, C> {
    pub async fn prune(&self) -> Result<u64, FactorError> {
        committed!((*self.backend), |repo: Factors| repo.prune().await)
    }
}

impl<C: Clock + Clone + Send + Sync + 'static> Each<'_, domain::Sessions, C> {
    pub async fn prune(&self) -> Result<u64, SessionError> {
        committed!((*self.backend), |repo: Sessions| repo.prune().await)
    }
}

impl<C: Clock + Clone + Send + Sync + 'static> Each<'_, domain::Flows, C> {
    pub async fn prune(&self) -> Result<u64, FlowError> {
        committed!((*self.backend), |repo: Flows| repo.prune().await)
    }
}

impl<C: Clock + Clone + Send + Sync + 'static> Each<'_, domain::Resets, C> {
    pub async fn prune(&self) -> Result<u64, ResetError> {
        committed!((*self.backend), |repo: Resets| repo.prune().await)
    }
}

impl<C: Send + Sync + 'static> Each<'_, domain::Accounts, C> {
    pub async fn create(
        &self,
        email: &Email,
        locked: &PasswordHash,
    ) -> Result<AccountId, AccountError> {
        committed!((*self.backend), |repo: Accounts| repo
            .create(email, locked)
            .await)
    }

    pub async fn linked(
        &self,
        identity: &ResolvedIdentity,
    ) -> Result<Option<AccountId>, AccountError> {
        committed!((*self.backend), |repo: Accounts| repo
            .linked(identity)
            .await)
    }

    pub async fn established(
        &self,
        identity: &ResolvedIdentity,
    ) -> Result<AccountId, AccountError> {
        committed!((*self.backend), |repo: Accounts| repo
            .established(identity)
            .await)
    }

    pub async fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> Result<AccountId, AccountError> {
        committed!((*self.backend), |repo: Accounts| repo
            .provision(identity, policy)
            .await)
    }
}
