use super::errors::BadSchedule;
use super::value_objects::{
    Day, Handoff, MAX_ROTATION, NoticeId, OverrideId, PageId, PageState, RotationLength,
    ScheduleId, ScheduleName, Timezone,
};
use crate::alerts::{DeliveryStatus, EventId, RuleId};
use crate::identity::{AccountId, Email};
use crate::incidents::{IncidentId, Person};
use crate::organisation::TeamId;
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rotation(Vec<AccountId>);

impl Rotation {
    pub fn new(people: Vec<AccountId>) -> Result<Self, BadSchedule> {
        let mut seen: Vec<AccountId> = Vec::with_capacity(people.len());
        for person in &people {
            if seen.contains(person) {
                return Err(BadSchedule::Rotation { max: MAX_ROTATION });
            }
            seen.push(*person);
        }
        if people.len() > MAX_ROTATION {
            return Err(BadSchedule::Rotation { max: MAX_ROTATION });
        }
        Ok(Self(people))
    }

    pub fn people(&self) -> &[AccountId] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleDraft {
    pub name: ScheduleName,
    pub timezone: Timezone,
    pub rotation: Rotation,
    pub handoff: Handoff,
    pub length: RotationLength,
    pub starts_on: Day,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub id: ScheduleId,
    pub org: TenantId,
    pub team: TeamId,
    pub name: ScheduleName,
    pub timezone: Timezone,
    pub rotation: Rotation,
    pub handoff: Handoff,
    pub length: RotationLength,
    pub starts_on: Day,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchedulePatch {
    pub name: Option<ScheduleName>,
    pub timezone: Option<Timezone>,
    pub rotation: Option<Rotation>,
    pub handoff: Option<Handoff>,
    pub length: Option<RotationLength>,
    pub starts_on: Option<Day>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Override {
    pub id: OverrideId,
    pub schedule: ScheduleId,
    pub account: AccountId,
    pub from_ms: u64,
    pub to_ms: u64,
}

impl Override {
    pub fn covers(&self, at_ms: u64) -> bool {
        self.from_ms <= at_ms && at_ms < self.to_ms
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shift {
    pub account: AccountId,
    pub until_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rostered {
    pub schedule: Schedule,
    pub overrides: Vec<Override>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnCallNow {
    pub person: Option<Person>,
    pub until_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnCallSchedule {
    pub schedule: ScheduleId,
    pub name: ScheduleName,
    pub team: TeamId,
    pub now: OnCallNow,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OnCallStatus {
    pub on_call: bool,
    pub schedules: Vec<ScheduleName>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageSource {
    Alert { rule: RuleId, event: EventId },
    Incident(IncidentId),
}

impl PageSource {
    pub fn origin(self) -> Origin {
        match self {
            Self::Alert { rule, .. } => Origin::Rule(rule),
            Self::Incident(incident) => Origin::Incident(incident),
        }
    }

    pub fn kind(self) -> &'static str {
        match self {
            Self::Alert { .. } => "alert",
            Self::Incident(_) => "incident",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    Rule(RuleId),
    Incident(IncidentId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub id: PageId,
    pub org: TenantId,
    pub schedule: ScheduleId,
    pub target: AccountId,
    pub source: PageSource,
    pub headline: Box<str>,
    pub state: PageState,
    pub step: u32,
    pub created_ms: u64,
    pub escalate_at_ms: Option<u64>,
    pub acknowledged_by: Option<AccountId>,
    pub acknowledged_ms: Option<u64>,
    pub resolved_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Escalation {
    Next {
        target: AccountId,
        step: u32,
        escalate_at_ms: u64,
    },
    Admins {
        step: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageView {
    pub page: Page,
    pub schedule: Option<ScheduleName>,
    pub target: Person,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub id: NoticeId,
    pub org: TenantId,
    pub page: PageId,
    pub step: u32,
    pub to: AccountId,
    pub attempt: u32,
    pub next_at_ms: u64,
    pub status: DeliveryStatus,
    pub last_error: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDispatch {
    pub page: PageId,
    pub org: TenantId,
    pub source: PageSource,
    pub headline: Box<str>,
    pub step: u32,
    pub at_ms: u64,
    pub ack: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedNotice {
    pub notice: Notice,
    pub email: Email,
    pub dispatch: PageDispatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paged {
    Skipped,
    Worked {
        escalated: usize,
        sent: usize,
        retried: usize,
        abandoned: usize,
    },
}
