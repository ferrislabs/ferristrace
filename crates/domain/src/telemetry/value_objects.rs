use core::fmt;
use std::collections::BTreeMap;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::billing::BillingHour;
use super::entities::{LogRecord, Span};
use super::errors::{BadRetention, BadSeverity, InvalidId};
use crate::catalogue::ProjectId;
use crate::secret::split_key;

macro_rules! identifier {
    ($name:ident, $width:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name([u8; $width]);

        impl $name {
            pub const WIDTH: usize = $width;

            pub fn as_bytes(&self) -> &[u8; $width] {
                &self.0
            }
        }

        impl TryFrom<&[u8]> for $name {
            type Error = InvalidId;

            fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
                let sized: [u8; $width] = bytes.try_into().map_err(|_| InvalidId::Length {
                    expected: $width,
                    actual: bytes.len(),
                })?;
                if sized.iter().all(|byte| *byte == 0) {
                    return Err(InvalidId::Zero);
                }
                Ok(Self(sized))
            }
        }
    };
}

identifier!(TraceId, 16);

identifier!(SpanId, 8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanKind {
    #[default]
    Unspecified,
    Internal,
    Server,
    Client,
    Producer,
    Consumer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanStatus {
    #[default]
    Unset,
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanEvent {
    pub time_unix_nanos: u64,
    pub name: Box<str>,
    pub attributes: BTreeMap<Box<str>, Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    pub spans: Vec<Span>,
    pub decompressed: DecompressedBytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPayload {
    pub records: Vec<LogRecord>,
    pub decompressed: DecompressedBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl Severity {
    pub const ALL: [Self; 6] = [
        Self::Trace,
        Self::Debug,
        Self::Info,
        Self::Warn,
        Self::Error,
        Self::Fatal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Fatal => "fatal",
        }
    }

    pub fn rank(self) -> usize {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
            Self::Fatal => 5,
        }
    }
}

impl TryFrom<&str> for Severity {
    type Error = BadSeverity;

    fn try_from(spelling: &str) -> Result<Self, BadSeverity> {
        Self::ALL
            .into_iter()
            .find(|severity| severity.name() == spelling)
            .ok_or(BadSeverity)
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct IngestKey(String);

impl IngestKey {
    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }

    pub fn split(&self) -> Option<(&str, &str)> {
        split_key(&self.0)
    }
}

impl fmt::Debug for IngestKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IngestKey(redacted)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TenantId(Uuid);

impl TenantId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

pub const MIN_RETENTION_DAYS: u32 = 1;
pub const MAX_RETENTION_DAYS: u32 = 3650;
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RetentionDays(u32);

impl RetentionDays {
    pub fn get(self) -> u32 {
        self.0
    }

    pub fn seconds(self) -> u64 {
        u64::from(self.0) * SECONDS_PER_DAY
    }

    #[must_use]
    pub fn saturating(days: u32) -> Self {
        Self(days.clamp(MIN_RETENTION_DAYS, MAX_RETENTION_DAYS))
    }
}

impl TryFrom<u32> for RetentionDays {
    type Error = BadRetention;

    fn try_from(days: u32) -> Result<Self, Self::Error> {
        if (MIN_RETENTION_DAYS..=MAX_RETENTION_DAYS).contains(&days) {
            Ok(Self(days))
        } else {
            Err(BadRetention {
                min: MIN_RETENTION_DAYS,
                max: MAX_RETENTION_DAYS,
            })
        }
    }
}

impl fmt::Display for RetentionDays {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    pub tenant: TenantId,
    pub project: Option<ProjectId>,
    pub retention: Option<RetentionDays>,
}

impl Scope {
    pub fn of(tenant: TenantId) -> Self {
        Self {
            tenant,
            project: None,
            retention: None,
        }
    }

    #[must_use]
    pub fn retained_for(mut self, retention: Option<RetentionDays>) -> Self {
        self.retention = retention;
        self
    }

    #[must_use]
    pub fn in_project(mut self, project: Option<ProjectId>) -> Self {
        self.project = project;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Within,
    Overage { over_by: DecompressedBytes },
    Capped { allowance: DecompressedBytes },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct DecompressedBytes(u64);

impl DecompressedBytes {
    pub fn new(count: u64) -> Self {
        Self(count)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Consumed {
    pub started_at_unix_nanos: u64,
    pub ingested_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumption {
    pub ingested_bytes: u64,
    pub included_bytes: Option<u64>,
    pub hours: Vec<Consumed>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HourTotal {
    pub tenant: TenantId,
    pub hour: BillingHour,
    pub ingested: DecompressedBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthTotal {
    pub tenant: TenantId,
    pub ingested: DecompressedBytes,
}
