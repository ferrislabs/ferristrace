use core::future::Future;

use ferristrace_domain::{
    AccountDirectory, Accounts, AlertChannels, AlertDuty, AlertEvents, AlertOutbox, AlertRules,
    AlertStates, ApiKeys, AuditTrail, AuthProvider, ControlPlane, ExportSource, FactorStore, Gate,
    IncidentAttachments, IncidentTimeline, Incidents, IssueDecisions, Memberships, OidcFlows, Orgs,
    Pages, Projects, ResetStore, Roll, Schedules, SessionRoll, SessionStore, Squads, UsageHistory,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settle {
    OnSuccess,
    Always,
}

pub trait UnitOfWork: Send + Sync + 'static {
    type Tx: Clone + Send + Sync + 'static;

    fn transaction<T, E, F, Fut>(
        &self,
        unavailable: impl Fn() -> E + Send,
        settle: Settle,
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
    pub struct AlertChannels;
    #[derive(Debug)]
    pub struct AlertDuty;
    #[derive(Debug)]
    pub struct AlertEvents;
    #[derive(Debug)]
    pub struct AlertOutbox;
    #[derive(Debug)]
    pub struct AlertRules;
    #[derive(Debug)]
    pub struct AlertStates;
    #[derive(Debug)]
    pub struct ApiKeys;
    #[derive(Debug)]
    pub struct Audit;
    #[derive(Debug)]
    pub struct ControlPlane;
    #[derive(Debug)]
    pub struct Decisions;
    #[derive(Debug)]
    pub struct Export;
    #[derive(Debug)]
    pub struct Factors;
    #[derive(Debug)]
    pub struct Flows;
    #[derive(Debug)]
    pub struct IncidentAttachments;
    #[derive(Debug)]
    pub struct IncidentTimeline;
    #[derive(Debug)]
    pub struct Incidents;
    #[derive(Debug)]
    pub struct LocalPasswords;
    #[derive(Debug)]
    pub struct Memberships;
    #[derive(Debug)]
    pub struct Orgs;
    #[derive(Debug)]
    pub struct Pages;
    #[derive(Debug)]
    pub struct Projects;
    #[derive(Debug)]
    pub struct Resets;
    #[derive(Debug)]
    pub struct Roster;
    #[derive(Debug)]
    pub struct Schedules;
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

pub trait RecordingRepos: RepoFor<domain::Audit, Repo: AuditTrail> {}

impl<R> RecordingRepos for R where R: RepoFor<domain::Audit, Repo: AuditTrail> {}

pub trait AuditRepos: StandingRepos + RecordingRepos {}

impl<R> AuditRepos for R where R: StandingRepos + RecordingRepos {}

pub trait ExportRepos:
    StandingRepos + RecordingRepos + RepoFor<domain::Export, Repo: ExportSource>
{
}

impl<R> ExportRepos for R where
    R: StandingRepos + RecordingRepos + RepoFor<domain::Export, Repo: ExportSource>
{
}

pub trait ReadRepos: StandingRepos + RepoFor<domain::ApiKeys, Repo: ApiKeys> {}

impl<R> ReadRepos for R where R: StandingRepos + RepoFor<domain::ApiKeys, Repo: ApiKeys> {}

pub trait AccessRepos:
    StandingRepos
    + RecordingRepos
    + RepoFor<domain::ApiKeys, Repo: ApiKeys>
    + RepoFor<domain::Projects, Repo: Projects>
{
}

impl<R> AccessRepos for R where
    R: StandingRepos
        + RecordingRepos
        + RepoFor<domain::ApiKeys, Repo: ApiKeys>
        + RepoFor<domain::Projects, Repo: Projects>
{
}

pub trait OrgRepos: StandingRepos + RecordingRepos + RepoFor<domain::Orgs, Repo: Orgs> {}

impl<R> OrgRepos for R where R: StandingRepos + RecordingRepos + RepoFor<domain::Orgs, Repo: Orgs> {}

pub trait CatalogueRepos:
    StandingRepos + RecordingRepos + RepoFor<domain::Projects, Repo: Projects>
{
}

impl<R> CatalogueRepos for R where
    R: StandingRepos + RecordingRepos + RepoFor<domain::Projects, Repo: Projects>
{
}

pub trait RosterRepos:
    StandingRepos + RecordingRepos + RepoFor<domain::Roster, Repo: Roll>
{
}

impl<R> RosterRepos for R where
    R: StandingRepos + RecordingRepos + RepoFor<domain::Roster, Repo: Roll>
{
}

pub trait TeamRepos: StandingRepos + RecordingRepos + RepoFor<domain::Teams, Repo: Squads> {}

impl<R> TeamRepos for R where
    R: StandingRepos + RecordingRepos + RepoFor<domain::Teams, Repo: Squads>
{
}

pub trait UsageRepos: StandingRepos + RepoFor<domain::UsageHistory, Repo: UsageHistory> {}

impl<R> UsageRepos for R where R: StandingRepos + RepoFor<domain::UsageHistory, Repo: UsageHistory> {}

pub trait IssueRepos:
    ReadRepos + RecordingRepos + RepoFor<domain::Decisions, Repo: IssueDecisions>
{
}

impl<R> IssueRepos for R where
    R: ReadRepos + RecordingRepos + RepoFor<domain::Decisions, Repo: IssueDecisions>
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

pub trait ProvisioningRepos:
    RecordingRepos + RepoFor<domain::ControlPlane, Repo: ControlPlane>
{
}

impl<R> ProvisioningRepos for R where
    R: RecordingRepos + RepoFor<domain::ControlPlane, Repo: ControlPlane>
{
}

pub trait WatchRepos:
    RepoFor<domain::AlertRules, Repo: AlertRules>
    + RepoFor<domain::AlertStates, Repo: AlertStates>
    + RepoFor<domain::AlertEvents, Repo: AlertEvents>
    + RepoFor<domain::AlertOutbox, Repo: AlertOutbox>
    + RepoFor<domain::AlertChannels, Repo: AlertChannels>
    + RepoFor<domain::AlertDuty, Repo: AlertDuty>
    + RepoFor<domain::Incidents, Repo: Incidents>
    + RepoFor<domain::IncidentTimeline, Repo: IncidentTimeline>
    + RepoFor<domain::Schedules, Repo: Schedules>
    + RepoFor<domain::Pages, Repo: Pages>
{
}

impl<R> WatchRepos for R where
    R: RepoFor<domain::AlertRules, Repo: AlertRules>
        + RepoFor<domain::AlertStates, Repo: AlertStates>
        + RepoFor<domain::AlertEvents, Repo: AlertEvents>
        + RepoFor<domain::AlertOutbox, Repo: AlertOutbox>
        + RepoFor<domain::AlertChannels, Repo: AlertChannels>
        + RepoFor<domain::AlertDuty, Repo: AlertDuty>
        + RepoFor<domain::Incidents, Repo: Incidents>
        + RepoFor<domain::IncidentTimeline, Repo: IncidentTimeline>
        + RepoFor<domain::Schedules, Repo: Schedules>
        + RepoFor<domain::Pages, Repo: Pages>
{
}

pub trait AlertRepos:
    ReadRepos + RecordingRepos + WatchRepos + RepoFor<domain::Projects, Repo: Projects>
{
}

impl<R> AlertRepos for R where
    R: ReadRepos + RecordingRepos + WatchRepos + RepoFor<domain::Projects, Repo: Projects>
{
}

pub trait IncidentRepos:
    AlertRepos
    + RepoFor<domain::IncidentAttachments, Repo: IncidentAttachments>
    + RepoFor<domain::Roster, Repo: Roll>
{
}

impl<R> IncidentRepos for R where
    R: AlertRepos
        + RepoFor<domain::IncidentAttachments, Repo: IncidentAttachments>
        + RepoFor<domain::Roster, Repo: Roll>
{
}

pub trait OncallRepos: IncidentRepos + RepoFor<domain::Teams, Repo: Squads> {}

impl<R> OncallRepos for R where R: IncidentRepos + RepoFor<domain::Teams, Repo: Squads> {}

pub trait Repositories:
    ReadRepos
    + AlertRepos
    + IncidentRepos
    + OncallRepos
    + AuditRepos
    + ExportRepos
    + AccessRepos
    + OrgRepos
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
    R: ReadRepos
        + AlertRepos
        + IncidentRepos
        + OncallRepos
        + AuditRepos
        + ExportRepos
        + AccessRepos
        + OrgRepos
        + CatalogueRepos
        + RosterRepos
        + TeamRepos
        + UsageRepos
        + IssueRepos
        + IdentityRepos
        + ProvisioningRepos
{
}
