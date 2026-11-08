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

pub fn views<C>(backend: &Postgres<C>) -> Each<'_, domain::Views, C> {
    Each::over(backend)
}

pub fn usage<C>(backend: &Postgres<C>) -> Each<'_, domain::UsageHistory, C> {
    Each::over(backend)
}

pub fn decisions<C>(backend: &Postgres<C>) -> Each<'_, domain::Decisions, C> {
    Each::over(backend)
}

pub fn api_keys<C>(backend: &Postgres<C>) -> Each<'_, domain::ApiKeys, C> {
    Each::over(backend)
}

pub fn agents<C>(backend: &Postgres<C>) -> Each<'_, domain::Agents, C> {
    Each::over(backend)
}

pub fn audit<C>(backend: &Postgres<C>) -> Each<'_, domain::Audit, C> {
    Each::over(backend)
}

pub fn export<C>(backend: &Postgres<C>) -> Each<'_, domain::Export, C> {
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
    fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError>;
});

forward!(Sessions: SessionSwitch {
    fn switch(&self, token: &SessionToken, org: TenantId) -> Result<(), SessionError>;
});

forward!(Memberships: Memberships {
    fn of(&self, account: AccountId) -> Result<Vec<Membership>, WhoamiError>;
});

forward!(Accounts: Accounts {
    fn rename(&self, account: AccountId, name: Option<&AccountName>) -> Result<(), StoreError>;
    fn portrait_of(&self, account: AccountId) -> Result<Option<Portrait>, StoreError>;
    fn set_portrait(&self, account: AccountId, portrait: Option<&Portrait>) -> Result<(), StoreError>;
    fn preferences_of(&self, account: AccountId) -> Result<Option<Preferences>, StoreError>;
    fn set_preferences(&self, account: AccountId, preferences: &Preferences) -> Result<(), StoreError>;
});

forward!(Accounts: AccountDirectory {
    fn provision(&self, identity: &ResolvedIdentity, policy: ProvisionPolicy) -> Result<AccountId, AuthError>;
});

forward!(Orgs: Orgs {
    fn only(&self, org: TenantId) -> Result<Option<Organisation>, StoreError>;
    fn rename(&self, org: TenantId, name: &OrgName) -> Result<(), StoreError>;
    fn disband(&self, org: TenantId) -> Result<bool, StoreError>;
    fn set_logo(&self, org: TenantId, logo: Option<&Portrait>) -> Result<(), StoreError>;
    fn set_retention(&self, org: TenantId, retention: Option<RetentionDays>) -> Result<(), StoreError>;
    fn set_timezone(&self, org: TenantId, timezone: &OrgTimezone) -> Result<(), StoreError>;
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
    fn issue(&self, org: TenantId, project: Option<ProjectId>, name: &KeyName, scopes: &KeyScopes, by: AccountId, ttl: KeyTtl) -> Result<IssuedKey, StoreError>;
    fn revoke(&self, org: TenantId, id: KeyId) -> Result<bool, StoreError>;
    fn grant_for(&self, secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError>;
    fn agent_standing(&self, org: TenantId, client_id: &str) -> Result<Option<RegisteredAgent>, StoreError>;
    fn agent_used(&self, org: TenantId, agent: AgentId, entry: &AuditEntry) -> Result<(), StoreError>;
});

forward!(Agents: Agents {
    fn list(&self, org: TenantId) -> Result<Vec<RegisteredAgent>, StoreError>;
    fn register(&self, org: TenantId, enrolment: &AgentEnrolment, by: AccountId) -> Result<Option<RegisteredAgent>, StoreError>;
    fn revoke(&self, org: TenantId, id: AgentId) -> Result<Option<RegisteredAgent>, StoreError>;
});

forward!(Audit: AuditTrail {
    fn record(&self, entry: &AuditEntry) -> Result<(), StoreError>;
    fn list(&self, org: TenantId, filter: &AuditFilter, page: PageSize) -> Result<AuditPage, StoreError>;
});

forward!(Export: ExportSource {
    fn snapshot(&self, org: TenantId) -> Result<Snapshot, StoreError>;
});

forward!(Roster: Roll {
    fn of(&self, org: TenantId) -> Result<Vec<Member>, StoreError>;
    fn grant(&self, org: TenantId, account: AccountId, role: OrgRole) -> Result<Option<Member>, StoreError>;
    fn remove(&self, org: TenantId, account: AccountId) -> Result<bool, StoreError>;
    fn standing_of(&self, email: &Email) -> Result<Option<Holder>, StoreError>;
    fn create_account(&self, email: &Email) -> Result<AccountId, StoreError>;
    fn join(&self, org: TenantId, account: AccountId, role: OrgRole) -> Result<Option<Joined>, StoreError>;
});

forward!(Teams: Squads {
    fn of(&self, org: TenantId) -> Result<Vec<Team>, StoreError>;
    fn create(&self, org: TenantId, name: &TeamName) -> Result<Named, StoreError>;
    fn rename(&self, org: TenantId, team: TeamId, name: &TeamName) -> Result<Named, StoreError>;
    fn disband(&self, org: TenantId, team: TeamId) -> Result<bool, StoreError>;
    fn seat(&self, org: TenantId, team: TeamId, members: &[AccountId]) -> Result<bool, StoreError>;
    fn own(&self, org: TenantId, team: TeamId, projects: &[ProjectId]) -> Result<bool, StoreError>;
});

forward!(Views: ExplorerViews {
    fn visible(&self, org: TenantId, signal: ViewSignal, to: AccountId) -> Result<Vec<View>, StoreError>;
    fn find(&self, org: TenantId, id: ViewId) -> Result<Option<View>, StoreError>;
    fn create(&self, org: TenantId, by: AccountId, view: &NewView) -> Result<Creation, StoreError>;
    fn change(&self, org: TenantId, id: ViewId, change: &ViewChange) -> Result<Saving, StoreError>;
    fn delete(&self, org: TenantId, id: ViewId) -> Result<bool, StoreError>;
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

forward!(ControlPlane: ControlPlane {
    fn founded(&self) -> Result<Option<TenantId>, ProvisionError>;
    fn found(&self, name: &OrgName, owner: &Email) -> Result<TenantId, ProvisionError>;
    fn grant(&self, org: TenantId, member: &Email, role: OrgRole) -> Result<(), ProvisionError>;
    fn account_for(&self, email: &Email) -> Result<Option<AccountId>, ProvisionError>;
    fn create_account(&self, email: &Email) -> Result<AccountId, ProvisionError>;
    fn claim_project(&self, org: TenantId, name: &ProjectName) -> Result<ProjectId, ProvisionError>;
    fn only_project(&self, org: TenantId) -> Result<Option<ProjectId>, ProvisionError>;
    fn issue_key(&self, org: TenantId, project: Option<ProjectId>) -> Result<Minted, ProvisionError>;
    fn revoke_key(&self, prefix: &str) -> Result<Option<Revoked>, ProvisionError>;
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

impl<C: Send + Sync + 'static> Each<'_, domain::Accounts, C> {
    pub async fn create(&self, email: &Email) -> Result<AccountId, AccountError> {
        committed!((*self.backend), |repo: Accounts| repo.create(email).await)
    }

    pub async fn linked(
        &self,
        identity: &ResolvedIdentity,
    ) -> Result<Option<AccountId>, AccountError> {
        committed!((*self.backend), |repo: Accounts| repo
            .linked(identity)
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

pub async fn signed_in(
    pool: &sqlx::PgPool,
    account: AccountId,
    org: Option<TenantId>,
) -> SessionToken {
    let row: (String,) = sqlx::query_as("SELECT email FROM accounts WHERE id = $1")
        .bind(account.as_uuid())
        .fetch_one(pool)
        .await
        .expect("the account exists");
    let identity = ResolvedIdentity {
        issuer: Issuer::new("https://issuer.ferristrace.test").expect("an issuer"),
        subject: Subject::try_from(account.to_string().as_str()).expect("a subject"),
        email: Email::try_from(row.0.as_str()).expect("an address"),
        email_verified: true,
    };
    sqlx::query(
        "INSERT INTO identity_links (issuer, subject, account_id) VALUES ($1, $2, $3) \
         ON CONFLICT DO NOTHING",
    )
    .bind(identity.issuer.as_str())
    .bind(identity.subject.as_str())
    .bind(account.as_uuid())
    .execute(pool)
    .await
    .expect("the identity links");
    if let Some(org) = org {
        sqlx::query("UPDATE accounts SET active_org_id = $2 WHERE id = $1")
            .bind(account.as_uuid())
            .bind(org.as_uuid())
            .execute(pool)
            .await
            .expect("the organisation is active");
    }
    SessionToken::verified(identity, UnixSeconds::new(4_102_444_800))
}
