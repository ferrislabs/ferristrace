use super::value_objects::{
    ComponentId, ComponentName, ComponentStatus, Intro, LetterId, PageName, ServiceName, Slug,
    StatusPageId, SubscriberId, Theme, UpdateKind,
};
use crate::alerts::{AlertSeverity, RuleId};
use crate::catalogue::ProjectId;
use crate::identity::Email;
use crate::incidents::{IncidentId, IncidentSeverity, IncidentStatus, IncidentTitle};
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusPage {
    pub id: StatusPageId,
    pub org: TenantId,
    pub name: PageName,
    pub slug: Slug,
    pub public: bool,
    pub hide_from_search: bool,
    pub theme: Theme,
    pub intro: Option<Intro>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDraft {
    pub name: PageName,
    pub slug: Slug,
    pub public: bool,
    pub hide_from_search: bool,
    pub theme: Theme,
    pub intro: Option<Intro>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PagePatch {
    pub name: Option<PageName>,
    pub slug: Option<Slug>,
    pub public: Option<bool>,
    pub hide_from_search: Option<bool>,
    pub theme: Option<Theme>,
    pub intro: Option<Option<Intro>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Service {
        name: ServiceName,
        project: Option<ProjectId>,
    },
    Alert(RuleId),
    Manual,
}

impl Link {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Service { .. } => "service",
            Self::Alert(_) => "alert",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub id: ComponentId,
    pub page: StatusPageId,
    pub name: ComponentName,
    pub link: Link,
    pub manual_status: Option<ComponentStatus>,
    pub position: u32,
    pub public: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDraft {
    pub name: ComponentName,
    pub link: Link,
    pub manual_status: Option<ComponentStatus>,
    pub public: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentPatch {
    pub name: Option<ComponentName>,
    pub link: Option<Link>,
    pub manual_status: Option<Option<ComponentStatus>>,
    pub public: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    pub page: StatusPage,
    pub components: Vec<Component>,
    pub subscribers: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub page: StatusPage,
    pub subscribers: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saved {
    Done,
    SlugTaken,
    Gone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceLoad {
    pub name: Box<str>,
    pub project: Option<ProjectId>,
    pub requests: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    pub firing: Vec<(RuleId, AlertSeverity)>,
    pub services: Vec<ServiceLoad>,
}

impl Observed {
    pub fn firing(&self, rule: RuleId) -> Option<AlertSeverity> {
        self.firing
            .iter()
            .find(|(held, _)| *held == rule)
            .map(|(_, severity)| *severity)
    }

    pub fn load(&self, name: &ServiceName, project: Option<ProjectId>) -> Option<&ServiceLoad> {
        self.services
            .iter()
            .find(|load| &*load.name == name.as_str() && load.project == project)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicUpdate {
    pub at_ms: u64,
    pub kind: UpdateKind,
    pub text: Box<str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicIncident {
    pub id: IncidentId,
    pub title: IncidentTitle,
    pub status: IncidentStatus,
    pub severity: IncidentSeverity,
    pub opened_ms: u64,
    pub resolved_ms: Option<u64>,
    pub updates: Vec<PublicUpdate>,
    pub postmortem: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicComponent {
    pub name: ComponentName,
    pub status: ComponentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicView {
    pub name: PageName,
    pub slug: Slug,
    pub theme: Theme,
    pub intro: Option<Intro>,
    pub hide_from_search: bool,
    pub overall: ComponentStatus,
    pub components: Vec<PublicComponent>,
    pub incidents: Vec<PublicIncident>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicPostmortem {
    pub title: IncidentTitle,
    pub body: Box<str>,
    pub published_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscriber {
    pub id: SubscriberId,
    pub page: StatusPageId,
    pub email: Email,
    pub confirmed: bool,
    pub invited_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkNotice {
    Confirmed,
    Unsubscribed,
}

impl LinkNotice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Unsubscribed => "unsubscribed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landing {
    pub slug: Slug,
    pub notice: LinkNotice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub org: TenantId,
    pub incident: IncidentId,
    pub title: IncidentTitle,
    pub kind: UpdateKind,
    pub status: IncidentStatus,
    pub text: Box<str>,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub id: LetterId,
    pub page: StatusPageId,
    pub subscriber: SubscriberId,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bulletin {
    pub title: IncidentTitle,
    pub kind: UpdateKind,
    pub status: IncidentStatus,
    pub text: Box<str>,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LetterBody {
    Confirm,
    Update(Bulletin),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedLetter {
    pub id: LetterId,
    pub attempt: u32,
    pub page: StatusPageId,
    pub page_name: PageName,
    pub slug: Slug,
    pub subscriber: SubscriberId,
    pub email: Email,
    pub body: LetterBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Letter {
    pub page_name: PageName,
    pub slug: Slug,
    pub body: LetterBody,
    pub confirm: Option<Box<str>>,
    pub unsubscribe: Box<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heralded {
    Skipped,
    Worked {
        sent: usize,
        retried: usize,
        abandoned: usize,
    },
}
