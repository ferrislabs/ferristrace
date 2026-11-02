use super::value_objects::{
    AttachmentId, AttachmentKind, AttachmentLabel, CommentBody, EntryKind, IncidentId,
    IncidentScope, IncidentSeverity, IncidentStatus, IncidentTitle, Reference, TimelineId,
};
use crate::access::KeyId;
use crate::alerts::{AlertStatus, Labels, RuleId};
use crate::catalogue::ProjectId;
use crate::identity::{AccountId, Email};
use crate::oncall::{PageId, ScheduleId};
use crate::telemetry::TenantId;

pub const FORMER_MEMBER: &str = "a former member";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incident {
    pub id: IncidentId,
    pub org: TenantId,
    pub project: Option<ProjectId>,
    pub title: IncidentTitle,
    pub severity: IncidentSeverity,
    pub status: IncidentStatus,
    pub owner: Option<AccountId>,
    pub public: bool,
    pub opened_ms: u64,
    pub resolved_ms: Option<u64>,
    pub opened_from_alert: Option<RuleId>,
    pub paged_schedule: Option<ScheduleId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Author {
    Account(AccountId),
    ApiKey(KeyId),
    Agent(Box<str>),
    System,
}

impl Author {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Account(_) => "account",
            Self::ApiKey(_) => "api_key",
            Self::Agent(_) => "agent",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: AttachmentId,
    pub incident: IncidentId,
    pub reference: Reference,
    pub label: AttachmentLabel,
    pub at_ms: u64,
}

impl Attachment {
    pub fn kind(&self) -> AttachmentKind {
        self.reference.kind()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Opened {
        title: IncidentTitle,
        severity: IncidentSeverity,
        alert: Option<RuleId>,
    },
    Status {
        from: IncidentStatus,
        to: IncidentStatus,
    },
    Severity {
        from: IncidentSeverity,
        to: IncidentSeverity,
    },
    Owner {
        from: Option<AccountId>,
        to: Option<AccountId>,
    },
    Title {
        from: IncidentTitle,
        to: IncidentTitle,
    },
    Public {
        from: bool,
        to: bool,
    },
    Comment {
        body: CommentBody,
        public: bool,
    },
    Alert {
        rule: RuleId,
        from: AlertStatus,
        to: AlertStatus,
        value: Option<f64>,
        labels: Labels,
    },
    Attachment(Attachment),
    Paged {
        page: PageId,
        step: u32,
        target: Option<AccountId>,
    },
    Acknowledged {
        page: PageId,
    },
}

impl Change {
    pub fn kind(&self) -> EntryKind {
        match self {
            Self::Opened { .. } => EntryKind::Opened,
            Self::Status { .. } => EntryKind::Status,
            Self::Severity { .. } => EntryKind::Severity,
            Self::Owner { .. } => EntryKind::Owner,
            Self::Title { .. } => EntryKind::Title,
            Self::Public { .. } => EntryKind::Public,
            Self::Comment { .. } => EntryKind::Comment,
            Self::Alert { .. } => EntryKind::Alert,
            Self::Attachment(_) => EntryKind::Attachment,
            Self::Paged { .. } => EntryKind::Paged,
            Self::Acknowledged { .. } => EntryKind::Acknowledged,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEntry {
    pub id: TimelineId,
    pub incident: IncidentId,
    pub org: TenantId,
    pub author: Author,
    pub change: Change,
    pub at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncidentDraft {
    pub title: Option<IncidentTitle>,
    pub severity: Option<IncidentSeverity>,
    pub project: Option<ProjectId>,
    pub alert: Option<RuleId>,
    pub public: bool,
    pub page: Option<ScheduleId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncidentPatch {
    pub title: Option<IncidentTitle>,
    pub severity: Option<IncidentSeverity>,
    pub status: Option<IncidentStatus>,
    pub owner: Option<Option<AccountId>>,
    pub public: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IncidentFilter {
    pub scope: IncidentScope,
    pub project: Option<ProjectId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub account: AccountId,
    pub email: Option<Email>,
    pub name: Option<Box<str>>,
}

impl Person {
    pub fn label(&self) -> &str {
        match (&self.name, &self.email) {
            (Some(name), _) => name,
            (None, Some(email)) => email.as_str(),
            (None, None) => FORMER_MEMBER,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertRef {
    pub id: RuleId,
    pub name: Box<str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Briefed {
    pub incident: Incident,
    pub owner: Option<Person>,
    pub alert: Option<AlertRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chronicle {
    pub entries: Vec<TimelineEntry>,
    pub people: Vec<Person>,
    pub alerts: Vec<AlertRef>,
}

impl Chronicle {
    pub fn person(&self, account: AccountId) -> Option<&Person> {
        self.people.iter().find(|person| person.account == account)
    }

    pub fn alert(&self, rule: RuleId) -> Option<&AlertRef> {
        self.alerts.iter().find(|alert| alert.id == rule)
    }

    pub fn author_label<'a>(&'a self, author: &'a Author) -> &'a str {
        match author {
            Author::Account(account) => self.person(*account).map_or(FORMER_MEMBER, Person::label),
            Author::ApiKey(_) => "an API key",
            Author::Agent(name) => name,
            Author::System => "FerrisTrace",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dossier {
    pub briefed: Briefed,
    pub attachments: Vec<Attachment>,
    pub chronicle: Chronicle,
}
