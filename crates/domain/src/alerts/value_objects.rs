use core::fmt;
use core::fmt::Write as _;
use std::collections::BTreeMap;

use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::errors::{BadChannel, BadRule};
use crate::identity::Email;
use crate::metrics::{Aggregation, MAX_GROUP_BY};
use crate::oncall::ScheduleId;
use crate::telemetry::Severity;

pub const MIN_WINDOW_SECS: u64 = 60;
pub const MAX_WINDOW_SECS: u64 = 86_400;
pub const MAX_PENDING_SECS: u64 = 3_600;
pub const MAX_RULE_NAME_BYTES: usize = 128;
pub const MAX_CHANNEL_NAME_BYTES: usize = 64;
pub const MAX_ADDRESSES: usize = 20;
pub const MAX_URL_BYTES: usize = 2048;
pub const HISTORY_DAYS: u64 = 30;
pub const WEBHOOK_SECRET_BYTES: usize = 32;

macro_rules! identifier {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new(id: Uuid) -> Self {
                Self(id)
            }

            pub fn fresh() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

identifier!(RuleId);
identifier!(ChannelId);
identifier!(EventId);
identifier!(NotificationId);

macro_rules! spelled {
    ($name:ident { $($variant:ident => $spelling:literal),+ $(,)? }) => {
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $spelling),+
                }
            }

            pub fn parse(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|held| held.as_str() == text)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TraceMeasure {
    Count,
    ErrorRate,
    P50,
    P95,
    P99,
}

spelled!(TraceMeasure {
    Count => "count",
    ErrorRate => "error_rate",
    P50 => "p50",
    P95 => "p95",
    P99 => "p99",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Comparison {
    Above,
    AtLeast,
    Below,
    AtMost,
}

spelled!(Comparison {
    Above => ">",
    AtLeast => ">=",
    Below => "<",
    AtMost => "<=",
});

impl Comparison {
    pub fn holds(self, value: f64, threshold: f64) -> bool {
        match self {
            Self::Above => value > threshold,
            Self::AtLeast => value >= threshold,
            Self::Below => value < threshold,
            Self::AtMost => value <= threshold,
        }
    }

    pub fn worst(self, one: f64, other: f64) -> f64 {
        match self {
            Self::Above | Self::AtLeast => one.max(other),
            Self::Below | Self::AtMost => one.min(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertSeverity {
    Critical,
    Warning,
    Info,
}

spelled!(AlertSeverity {
    Critical => "critical",
    Warning => "warning",
    Info => "info",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoDataPolicy {
    Ok,
    NoData,
    Firing,
}

spelled!(NoDataPolicy {
    Ok => "ok",
    NoData => "no_data",
    Firing => "firing",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertStatus {
    Ok,
    Pending,
    Firing,
    NoData,
}

spelled!(AlertStatus {
    Ok => "ok",
    Pending => "pending",
    Firing => "firing",
    NoData => "no_data",
});

impl AlertStatus {
    pub fn gravity(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::NoData => 1,
            Self::Pending => 2,
            Self::Firing => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    Email,
    Webhook,
}

spelled!(ChannelKind {
    Email => "email",
    Webhook => "webhook",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeliveryStatus {
    Pending,
    Sent,
    Abandoned,
}

spelled!(DeliveryStatus {
    Pending => "pending",
    Sent => "sent",
    Abandoned => "abandoned",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Duty {
    Evaluate,
    Deliver,
    Escalate,
}

spelled!(Duty {
    Evaluate => "evaluate",
    Deliver => "deliver",
    Escalate => "escalate",
});

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Condition {
    op: Comparison,
    threshold: f64,
}

impl Condition {
    pub fn new(op: Comparison, threshold: f64) -> Result<Self, BadRule> {
        if threshold.is_finite() {
            Ok(Self { op, threshold })
        } else {
            Err(BadRule::Threshold)
        }
    }

    pub fn op(self) -> Comparison {
        self.op
    }

    pub fn threshold(self) -> f64 {
        self.threshold
    }

    pub fn holds(self, value: f64) -> bool {
        self.op.holds(value, self.threshold)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSignal {
    pub service: Option<Box<str>>,
    pub operation: Option<Box<str>>,
    pub environment: Option<Box<str>>,
    pub measure: TraceMeasure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSignal {
    pub services: Vec<Box<str>>,
    pub severities: Vec<Severity>,
    pub text: Option<Box<str>>,
    pub attributes: Vec<(Box<str>, Box<str>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricSignal {
    pub name: Box<str>,
    pub aggregation: Aggregation,
    pub group_by: Vec<Box<str>>,
    pub filter: BTreeMap<Box<str>, Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    Traces(TraceSignal),
    Logs(LogSignal),
    Metrics(MetricSignal),
}

impl Signal {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Traces(_) => "traces",
            Self::Logs(_) => "logs",
            Self::Metrics(_) => "metrics",
        }
    }

    pub fn groups(&self) -> bool {
        matches!(self, Self::Metrics(metric) if !metric.group_by.is_empty())
    }

    fn check(&self) -> Result<(), BadRule> {
        match self {
            Self::Traces(traces) => {
                if traces.operation.is_some() && traces.service.is_none() {
                    return Err(BadRule::OperationWithoutService);
                }
                Ok(())
            }
            Self::Logs(logs) => {
                if logs
                    .attributes
                    .iter()
                    .any(|(name, _)| name.trim().is_empty())
                {
                    return Err(BadRule::BlankKey);
                }
                Ok(())
            }
            Self::Metrics(metric) => {
                if metric.name.trim().is_empty() {
                    return Err(BadRule::MetricName);
                }
                if metric
                    .group_by
                    .iter()
                    .chain(metric.filter.keys())
                    .any(|key| key.trim().is_empty())
                {
                    return Err(BadRule::BlankKey);
                }
                let mut keys: Vec<&str> = metric
                    .group_by
                    .iter()
                    .chain(metric.filter.keys())
                    .map(|key| &**key)
                    .collect();
                keys.sort_unstable();
                keys.dedup();
                if keys.len() > MAX_GROUP_BY {
                    return Err(BadRule::GroupBy { max: MAX_GROUP_BY });
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionParts {
    pub signal: Signal,
    pub condition: Condition,
    pub window_secs: u64,
    pub pending_secs: u64,
    pub severity: AlertSeverity,
    pub no_data: NoDataPolicy,
    pub channels: Vec<ChannelId>,
    pub page: Option<ScheduleId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Definition(DefinitionParts);

impl Definition {
    pub fn new(mut parts: DefinitionParts) -> Result<Self, BadRule> {
        if !(MIN_WINDOW_SECS..=MAX_WINDOW_SECS).contains(&parts.window_secs) {
            return Err(BadRule::Window {
                min: MIN_WINDOW_SECS,
                max: MAX_WINDOW_SECS,
            });
        }
        if parts.pending_secs > MAX_PENDING_SECS {
            return Err(BadRule::Pending {
                max: MAX_PENDING_SECS,
            });
        }
        parts.signal.check()?;
        let mut seen = Vec::with_capacity(parts.channels.len());
        parts.channels.retain(|channel| {
            let fresh = !seen.contains(channel);
            seen.push(*channel);
            fresh
        });
        Ok(Self(parts))
    }

    pub fn parts(&self) -> &DefinitionParts {
        &self.0
    }

    pub fn into_parts(self) -> DefinitionParts {
        self.0
    }

    pub fn signal(&self) -> &Signal {
        &self.0.signal
    }

    pub fn condition(&self) -> Condition {
        self.0.condition
    }

    pub fn window_secs(&self) -> u64 {
        self.0.window_secs
    }

    pub fn pending_secs(&self) -> u64 {
        self.0.pending_secs
    }

    pub fn severity(&self) -> AlertSeverity {
        self.0.severity
    }

    pub fn no_data(&self) -> NoDataPolicy {
        self.0.no_data
    }

    pub fn channels(&self) -> &[ChannelId] {
        &self.0.channels
    }

    pub fn page(&self) -> Option<ScheduleId> {
        self.0.page
    }
}

fn bounded(chosen: &str, max: usize) -> Result<Box<str>, usize> {
    let kept = chosen.trim();
    if kept.is_empty() || kept.len() > max {
        return Err(max);
    }
    Ok(kept.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleName(Box<str>);

impl RuleName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for RuleName {
    type Error = BadRule;

    fn try_from(chosen: &str) -> Result<Self, BadRule> {
        bounded(chosen, MAX_RULE_NAME_BYTES)
            .map(Self)
            .map_err(|max| BadRule::Name { max })
    }
}

impl fmt::Display for RuleName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelName(Box<str>);

impl ChannelName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ChannelName {
    type Error = BadChannel;

    fn try_from(chosen: &str) -> Result<Self, BadChannel> {
        bounded(chosen, MAX_CHANNEL_NAME_BYTES)
            .map(Self)
            .map_err(|max| BadChannel::Name { max })
    }
}

impl fmt::Display for ChannelName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Labels(BTreeMap<Box<str>, Box<str>>);

impl Labels {
    pub fn new(pairs: BTreeMap<Box<str>, Box<str>>) -> Self {
        Self(pairs)
    }

    pub fn none() -> Self {
        Self::default()
    }

    pub fn key(&self) -> String {
        let mut key = String::new();
        for (name, value) in &self.0 {
            let _ = write!(key, "{}:{name}{}:{value}", name.len(), value.len());
        }
        key
    }

    pub fn pairs(&self) -> &BTreeMap<Box<str>, Box<str>> {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailList(Vec<Email>);

impl EmailList {
    pub fn addresses(&self) -> &[Email] {
        &self.0
    }

    pub fn target(&self) -> String {
        self.0
            .iter()
            .map(Email::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl TryFrom<&str> for EmailList {
    type Error = BadChannel;

    fn try_from(listed: &str) -> Result<Self, BadChannel> {
        let mut held: Vec<Email> = Vec::new();
        for part in listed.split([',', ';']).map(str::trim) {
            if part.is_empty() {
                continue;
            }
            let address = Email::try_from(part).map_err(|_| BadChannel::Address)?;
            if !held.contains(&address) {
                held.push(address);
            }
        }
        if held.is_empty() {
            return Err(BadChannel::NoAddress);
        }
        if held.len() > MAX_ADDRESSES {
            return Err(BadChannel::TooManyAddresses { max: MAX_ADDRESSES });
        }
        Ok(Self(held))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookUrl {
    url: Box<str>,
    host: Box<str>,
}

impl WebhookUrl {
    pub fn as_str(&self) -> &str {
        &self.url
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn is_https(&self) -> bool {
        self.url.starts_with("https://")
    }
}

impl TryFrom<&str> for WebhookUrl {
    type Error = BadChannel;

    fn try_from(given: &str) -> Result<Self, BadChannel> {
        let url = given.trim();
        if url.len() > MAX_URL_BYTES || url.contains(char::is_whitespace) {
            return Err(BadChannel::Url);
        }
        let lowered = url.to_ascii_lowercase();
        let rest = lowered
            .strip_prefix("https://")
            .or_else(|| lowered.strip_prefix("http://"))
            .ok_or(BadChannel::Url)?;
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.contains('@') {
            return Err(BadChannel::Url);
        }
        let host = if let Some(bracketed) = authority.strip_prefix('[') {
            let (inside, after) = bracketed.split_once(']').ok_or(BadChannel::Url)?;
            if !(after.is_empty() || after.starts_with(':')) {
                return Err(BadChannel::Url);
            }
            inside
        } else {
            authority.split(':').next().unwrap_or_default()
        };
        if host.is_empty() {
            return Err(BadChannel::Url);
        }
        Ok(Self {
            url: url.into(),
            host: host.trim_end_matches('.').into(),
        })
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct WebhookSecret(String);

impl WebhookSecret {
    pub fn generate() -> Self {
        let bytes: [u8; WEBHOOK_SECRET_BYTES] = rand::random();
        Self(hex::encode(bytes))
    }

    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for WebhookSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WebhookSecret(redacted)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Email(EmailList),
    Webhook(WebhookUrl),
}

impl Destination {
    pub fn parse(kind: ChannelKind, target: &str) -> Result<Self, BadChannel> {
        match kind {
            ChannelKind::Email => EmailList::try_from(target).map(Self::Email),
            ChannelKind::Webhook => WebhookUrl::try_from(target).map(Self::Webhook),
        }
    }

    pub fn kind(&self) -> ChannelKind {
        match self {
            Self::Email(_) => ChannelKind::Email,
            Self::Webhook(_) => ChannelKind::Webhook,
        }
    }

    pub fn target(&self) -> String {
        match self {
            Self::Email(list) => list.target(),
            Self::Webhook(url) => url.as_str().to_owned(),
        }
    }
}
