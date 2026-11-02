use super::culprits::Culprits;
use super::errors::DeliveryError;
use super::value_objects::{
    AlertSeverity, AlertStatus, ChannelId, ChannelKind, ChannelName, Condition, Definition,
    DeliveryStatus, Destination, EventId, Labels, NoDataPolicy, NotificationId, RuleId, RuleName,
    Signal, WebhookSecret,
};
use crate::catalogue::ProjectId;
use crate::oncall::ScheduleId;
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq)]
pub struct RuleDraft {
    pub name: RuleName,
    pub project: Option<ProjectId>,
    pub enabled: bool,
    pub definition: Definition,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: RuleId,
    pub org: TenantId,
    pub name: RuleName,
    pub project: Option<ProjectId>,
    pub enabled: bool,
    pub definition: Definition,
    pub created_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RulePatch {
    pub name: Option<RuleName>,
    pub project: Option<Option<ProjectId>>,
    pub enabled: Option<bool>,
    pub signal: Option<Signal>,
    pub condition: Option<Condition>,
    pub window_secs: Option<u64>,
    pub pending_secs: Option<u64>,
    pub severity: Option<AlertSeverity>,
    pub no_data: Option<NoDataPolicy>,
    pub channels: Option<Vec<ChannelId>>,
    pub page: Option<Option<ScheduleId>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuleState {
    pub status: AlertStatus,
    pub value: Option<f64>,
    pub since_ms: u64,
    pub evaluated_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesState {
    pub labels: Labels,
    pub state: RuleState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    pub from: AlertStatus,
    pub to: AlertStatus,
    pub value: Option<f64>,
    pub at_ms: u64,
}

impl Transition {
    pub fn notifies(&self) -> bool {
        self.from != self.to && (self.to == AlertStatus::Firing || self.from == AlertStatus::Firing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overall {
    pub status: AlertStatus,
    pub value: Option<f64>,
    pub since_ms: u64,
    pub evaluated_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Watched {
    pub rule: Rule,
    pub series: Vec<SeriesState>,
}

impl Watched {
    pub fn overall(&self) -> Overall {
        let worst = self.series.iter().max_by_key(|series| {
            (
                series.state.status.gravity(),
                core::cmp::Reverse(series.state.since_ms),
            )
        });
        match worst {
            None => Overall {
                status: AlertStatus::Ok,
                value: None,
                since_ms: self.rule.created_ms,
                evaluated_ms: None,
            },
            Some(worst) => Overall {
                status: worst.state.status,
                value: worst.state.value,
                since_ms: worst.state.since_ms,
                evaluated_ms: self
                    .series
                    .iter()
                    .map(|series| series.state.evaluated_ms)
                    .max(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertEvent {
    pub id: EventId,
    pub rule: RuleId,
    pub org: TenantId,
    pub labels: Labels,
    pub from: AlertStatus,
    pub to: AlertStatus,
    pub value: Option<f64>,
    pub at_ms: u64,
    pub context: Option<Culprits>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: NotificationId,
    pub org: TenantId,
    pub rule: RuleId,
    pub event: EventId,
    pub channel: ChannelId,
    pub attempt: u32,
    pub next_at_ms: u64,
    pub status: DeliveryStatus,
    pub last_error: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dispatch {
    pub rule: RuleId,
    pub name: Box<str>,
    pub severity: AlertSeverity,
    pub org: TenantId,
    pub from: AlertStatus,
    pub to: AlertStatus,
    pub value: Option<f64>,
    pub labels: Labels,
    pub at_ms: u64,
    pub context: Option<Culprits>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelView {
    pub id: ChannelId,
    pub name: ChannelName,
    pub destination: Destination,
    pub has_secret: bool,
}

impl ChannelView {
    pub fn kind(&self) -> ChannelKind {
        self.destination.kind()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub id: ChannelId,
    pub name: ChannelName,
    pub destination: Destination,
    pub secret: Option<WebhookSecret>,
}

#[derive(Debug)]
pub struct IssuedChannel {
    pub channel: ChannelView,
    pub secret: Option<WebhookSecret>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Claimed {
    pub notification: Notification,
    pub dispatch: Dispatch,
    pub channel: Channel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settlement {
    Sent,
    Retry { next_at_ms: u64, error: Box<str> },
    Abandoned { error: Box<str> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub labels: Labels,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub step_nanos: u64,
    pub points: Vec<(u64, Option<f64>)>,
    pub threshold: f64,
    pub would_fire: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub channel: ChannelId,
    pub outcome: Result<(), DeliveryError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    Skipped,
    Evaluated {
        rules: usize,
        transitions: usize,
        failed: usize,
        pruned: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drained {
    Skipped,
    Delivered {
        sent: usize,
        retried: usize,
        abandoned: usize,
    },
}
