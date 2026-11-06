use core::future::Future;

use ferristrace_domain::{
    AccountDirectory, Accounts, Agents as AgentBook, AlertChannels, AlertDuty, AlertEvents,
    AlertOutbox, AlertRules, AlertStates, ApiKeys, AuditTrail, ControlPlane, DeployMarks,
    ExplorerViews, ExportSource, IncidentAttachments, IncidentTimeline, Incidents, IssueDecisions,
    Memberships, ModelPrices as ModelPriceBook, Orgs, Pages, Postmortems, Projects, Roll,
    Schedules, SessionStore, SessionSwitch, Squads, StatusOutbox, StatusPages, StatusSubscribers,
    UsageHistory,
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
    pub struct Agents;
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
    pub struct DeployMarks;
    #[derive(Debug)]
    pub struct Export;
    #[derive(Debug)]
    pub struct IncidentAttachments;
    #[derive(Debug)]
    pub struct IncidentTimeline;
    #[derive(Debug)]
    pub struct Incidents;
    #[derive(Debug)]
    pub struct Memberships;
    #[derive(Debug)]
    pub struct ModelPrices;
    #[derive(Debug)]
    pub struct Orgs;
    #[derive(Debug)]
    pub struct Pages;
    #[derive(Debug)]
    pub struct Postmortems;
    #[derive(Debug)]
    pub struct Projects;
    #[derive(Debug)]
    pub struct Roster;
    #[derive(Debug)]
    pub struct Schedules;
    #[derive(Debug)]
    pub struct Sessions;
    #[derive(Debug)]
    pub struct StatusOutbox;
    #[derive(Debug)]
    pub struct StatusPages;
    #[derive(Debug)]
    pub struct StatusSubscribers;
    #[derive(Debug)]
    pub struct Teams;
    #[derive(Debug)]
    pub struct UsageHistory;
    #[cfg(test)]
    #[derive(Debug)]
    pub struct Probe;
    #[derive(Debug)]
    pub struct Views;
}

pub trait StandingRepos:
    RepoFor<domain::Sessions, Repo: SessionStore> + RepoFor<domain::Memberships, Repo: Memberships>
{
}

impl<R> StandingRepos for R where
    R: RepoFor<domain::Sessions, Repo: SessionStore>
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

pub trait SiteRepos: ReadRepos + RepoFor<domain::Projects, Repo: Projects> {}

impl<R> SiteRepos for R where R: ReadRepos + RepoFor<domain::Projects, Repo: Projects> {}

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

pub trait AgentRepos:
    StandingRepos
    + RecordingRepos
    + RepoFor<domain::Agents, Repo: AgentBook>
    + RepoFor<domain::Projects, Repo: Projects>
{
}

impl<R> AgentRepos for R where
    R: StandingRepos
        + RecordingRepos
        + RepoFor<domain::Agents, Repo: AgentBook>
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

pub trait ViewRepos: StandingRepos + RepoFor<domain::Views, Repo: ExplorerViews> {}

impl<R> ViewRepos for R where R: StandingRepos + RepoFor<domain::Views, Repo: ExplorerViews> {}

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

pub trait SwitchRepos: StandingRepos + RepoFor<domain::Sessions, Repo: SessionSwitch> {}

impl<R> SwitchRepos for R where R: StandingRepos + RepoFor<domain::Sessions, Repo: SessionSwitch> {}

pub trait FoundRepos:
    RecordingRepos
    + RepoFor<domain::ControlPlane, Repo: ControlPlane>
    + RepoFor<domain::Sessions, Repo: SessionStore + SessionSwitch>
{
}

impl<R> FoundRepos for R where
    R: RecordingRepos
        + RepoFor<domain::ControlPlane, Repo: ControlPlane>
        + RepoFor<domain::Sessions, Repo: SessionStore + SessionSwitch>
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

pub trait DeployRepos: WatchRepos + RepoFor<domain::DeployMarks, Repo: DeployMarks> {}

impl<R> DeployRepos for R where R: WatchRepos + RepoFor<domain::DeployMarks, Repo: DeployMarks> {}

pub trait LlmRepos:
    ReadRepos + RecordingRepos + RepoFor<domain::ModelPrices, Repo: ModelPriceBook>
{
}

impl<R> LlmRepos for R where
    R: ReadRepos + RecordingRepos + RepoFor<domain::ModelPrices, Repo: ModelPriceBook>
{
}

pub trait AlertRepos:
    ReadRepos
    + RecordingRepos
    + WatchRepos
    + RepoFor<domain::Projects, Repo: Projects>
    + RepoFor<domain::ModelPrices, Repo: ModelPriceBook>
{
}

impl<R> AlertRepos for R where
    R: ReadRepos
        + RecordingRepos
        + WatchRepos
        + RepoFor<domain::Projects, Repo: Projects>
        + RepoFor<domain::ModelPrices, Repo: ModelPriceBook>
{
}

pub trait IncidentRepos:
    AlertRepos
    + RepoFor<domain::IncidentAttachments, Repo: IncidentAttachments>
    + RepoFor<domain::Roster, Repo: Roll>
    + RepoFor<domain::StatusOutbox, Repo: StatusOutbox>
{
}

impl<R> IncidentRepos for R where
    R: AlertRepos
        + RepoFor<domain::IncidentAttachments, Repo: IncidentAttachments>
        + RepoFor<domain::Roster, Repo: Roll>
        + RepoFor<domain::StatusOutbox, Repo: StatusOutbox>
{
}

pub trait OncallRepos: IncidentRepos + RepoFor<domain::Teams, Repo: Squads> {}

impl<R> OncallRepos for R where R: IncidentRepos + RepoFor<domain::Teams, Repo: Squads> {}

pub trait PostmortemRepos: IncidentRepos + RepoFor<domain::Postmortems, Repo: Postmortems> {}

impl<R> PostmortemRepos for R where
    R: IncidentRepos + RepoFor<domain::Postmortems, Repo: Postmortems>
{
}

pub trait StatusRepos:
    PostmortemRepos
    + RepoFor<domain::StatusPages, Repo: StatusPages>
    + RepoFor<domain::StatusSubscribers, Repo: StatusSubscribers>
{
}

impl<R> StatusRepos for R where
    R: PostmortemRepos
        + RepoFor<domain::StatusPages, Repo: StatusPages>
        + RepoFor<domain::StatusSubscribers, Repo: StatusSubscribers>
{
}

pub trait Repositories:
    ReadRepos
    + AlertRepos
    + IncidentRepos
    + OncallRepos
    + PostmortemRepos
    + StatusRepos
    + AuditRepos
    + ExportRepos
    + AccessRepos
    + AgentRepos
    + OrgRepos
    + CatalogueRepos
    + RosterRepos
    + TeamRepos
    + ViewRepos
    + UsageRepos
    + IssueRepos
    + ProvisioningRepos
    + SwitchRepos
    + FoundRepos
    + DeployRepos
    + LlmRepos
{
}

impl<R> Repositories for R where
    R: ReadRepos
        + AlertRepos
        + IncidentRepos
        + OncallRepos
        + PostmortemRepos
        + StatusRepos
        + AuditRepos
        + ExportRepos
        + AccessRepos
        + AgentRepos
        + OrgRepos
        + CatalogueRepos
        + RosterRepos
        + TeamRepos
        + ViewRepos
        + UsageRepos
        + IssueRepos
        + ProvisioningRepos
        + SwitchRepos
        + FoundRepos
        + DeployRepos
        + LlmRepos
{
}
