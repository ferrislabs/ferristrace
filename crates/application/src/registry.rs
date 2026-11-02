use core::future::Future;

use ferristrace_domain::{
    AccountDirectory, Accounts, AuthProvider, ControlPlane, FactorStore, Gate, IssueDecisions,
    Memberships, OidcFlows, Orgs, Projects, ResetStore, Roll, SessionRoll, SessionStore, Squads,
    UsageHistory,
};

pub trait UnitOfWork: Send + Sync + 'static {
    type Tx: Clone + Send + Sync + 'static;

    fn transaction<T, E, F, Fut>(
        &self,
        unavailable: impl Fn() -> E + Send,
        work: F,
    ) -> impl Future<Output = Result<T, E>> + Send
    where
        T: Send,
        E: Send,
        F: FnOnce(Self::Tx) -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send;
}

pub trait RepoFor<D>: UnitOfWork {
    type Repo: Send + Sync;

    fn build(&self, tx: &Self::Tx) -> Self::Repo;
}

pub mod domain {
    #[derive(Debug)]
    pub struct Accounts;
    #[derive(Debug)]
    pub struct ControlPlane;
    #[derive(Debug)]
    pub struct Decisions;
    #[derive(Debug)]
    pub struct Factors;
    #[derive(Debug)]
    pub struct Flows;
    #[derive(Debug)]
    pub struct LocalPasswords;
    #[derive(Debug)]
    pub struct Memberships;
    #[derive(Debug)]
    pub struct Orgs;
    #[derive(Debug)]
    pub struct Projects;
    #[derive(Debug)]
    pub struct Resets;
    #[derive(Debug)]
    pub struct Roster;
    #[derive(Debug)]
    pub struct Sessions;
    #[derive(Debug)]
    pub struct Teams;
    #[derive(Debug)]
    pub struct UsageHistory;
    #[cfg(test)]
    #[derive(Debug)]
    pub struct Probe;
}

pub trait StandingRepos:
    RepoFor<domain::Sessions, Repo: SessionStore + SessionRoll>
    + RepoFor<domain::Memberships, Repo: Memberships>
{
}

impl<R> StandingRepos for R where
    R: RepoFor<domain::Sessions, Repo: SessionStore + SessionRoll>
        + RepoFor<domain::Memberships, Repo: Memberships>
{
}

pub trait OrgRepos: StandingRepos + RepoFor<domain::Orgs, Repo: Orgs> {}

impl<R> OrgRepos for R where R: StandingRepos + RepoFor<domain::Orgs, Repo: Orgs> {}

pub trait CatalogueRepos: StandingRepos + RepoFor<domain::Projects, Repo: Projects> {}

impl<R> CatalogueRepos for R where R: StandingRepos + RepoFor<domain::Projects, Repo: Projects> {}

pub trait RosterRepos: StandingRepos + RepoFor<domain::Roster, Repo: Roll> {}

impl<R> RosterRepos for R where R: StandingRepos + RepoFor<domain::Roster, Repo: Roll> {}

pub trait TeamRepos: StandingRepos + RepoFor<domain::Teams, Repo: Squads> {}

impl<R> TeamRepos for R where R: StandingRepos + RepoFor<domain::Teams, Repo: Squads> {}

pub trait UsageRepos: StandingRepos + RepoFor<domain::UsageHistory, Repo: UsageHistory> {}

impl<R> UsageRepos for R where
    R: StandingRepos + RepoFor<domain::UsageHistory, Repo: UsageHistory>
{
}

pub trait IssueRepos: StandingRepos + RepoFor<domain::Decisions, Repo: IssueDecisions> {}

impl<R> IssueRepos for R where
    R: StandingRepos + RepoFor<domain::Decisions, Repo: IssueDecisions>
{
}

pub trait ProfileRepos:
    StandingRepos + RepoFor<domain::Accounts, Repo: Accounts + AccountDirectory>
{
}

impl<R> ProfileRepos for R where
    R: StandingRepos + RepoFor<domain::Accounts, Repo: Accounts + AccountDirectory>
{
}

pub trait IdentityRepos:
    StandingRepos
    + RepoFor<domain::Accounts, Repo: Accounts + AccountDirectory>
    + RepoFor<domain::Factors, Repo: FactorStore + Gate>
    + RepoFor<domain::Resets, Repo: ResetStore>
    + RepoFor<domain::Flows, Repo: OidcFlows>
    + RepoFor<domain::LocalPasswords, Repo: AuthProvider>
{
}

impl<R> IdentityRepos for R where
    R: StandingRepos
        + RepoFor<domain::Accounts, Repo: Accounts + AccountDirectory>
        + RepoFor<domain::Factors, Repo: FactorStore + Gate>
        + RepoFor<domain::Resets, Repo: ResetStore>
        + RepoFor<domain::Flows, Repo: OidcFlows>
        + RepoFor<domain::LocalPasswords, Repo: AuthProvider>
{
}

pub trait ProvisioningRepos: RepoFor<domain::ControlPlane, Repo: ControlPlane> {}

impl<R> ProvisioningRepos for R where R: RepoFor<domain::ControlPlane, Repo: ControlPlane> {}

pub trait Repositories:
    OrgRepos
    + CatalogueRepos
    + RosterRepos
    + TeamRepos
    + UsageRepos
    + IssueRepos
    + IdentityRepos
    + ProvisioningRepos
{
}

impl<R> Repositories for R where
    R: OrgRepos
        + CatalogueRepos
        + RosterRepos
        + TeamRepos
        + UsageRepos
        + IssueRepos
        + IdentityRepos
        + ProvisioningRepos
{
}
