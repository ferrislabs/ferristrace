use core::fmt;
use sha2::{Digest, Sha256};

use super::errors::BadFingerprint;
use super::fingerprint::normalise;
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::identity::AccountId;
use crate::telemetry::{SpanId, TraceId};

pub const EXCEPTION_EVENT: &str = "exception";

pub const PRINT_BYTES: usize = 16;

const DEFAULT_LIMIT: u32 = 50;

pub(super) const MAX_LIMIT: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; PRINT_BYTES]);

impl Fingerprint {
    pub fn new(bytes: [u8; PRINT_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; PRINT_BYTES] {
        &self.0
    }

    pub fn of(service: &str, exception_type: &str, message: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(service.as_bytes());
        hasher.update(b"\0");
        hasher.update(exception_type.as_bytes());
        hasher.update(b"\0");
        hasher.update(normalise(message).as_bytes());
        let whole: [u8; 32] = hasher.finalize().into();
        let mut print = [0; PRINT_BYTES];
        print.copy_from_slice(&whole[..PRINT_BYTES]);
        Self(print)
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl TryFrom<&str> for Fingerprint {
    type Error = BadFingerprint;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let bytes = hex::decode(text).map_err(|_| BadFingerprint)?;
        Ok(Self(bytes.try_into().map_err(|_| BadFingerprint)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub service: Box<str>,
    pub exception_type: Box<str>,
    pub message: Box<str>,
    pub stacktrace: Option<Box<str>>,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub time_unix_nanos: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Unresolved,
    Resolved,
    Ignored,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::Resolved => "resolved",
            Self::Ignored => "ignored",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "unresolved" => Some(Self::Unresolved),
            "resolved" => Some(Self::Resolved),
            "ignored" => Some(Self::Ignored),
            _ => None,
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Decision {
    pub status: Status,
    pub owner: Option<AccountId>,
    pub decided_at_unix_nanos: u64,
    pub decided_by: Option<AccountId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted {
    pub project: ProjectId,
    pub print: Fingerprint,
    pub exception_type: Box<str>,
    pub message: Box<str>,
    pub service: Box<str>,
    pub count: u64,
    pub first_seen_unix_nanos: u64,
    pub last_seen_unix_nanos: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    pub project: ProjectId,
    pub print: Fingerprint,
    pub decision: Decision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueFilter {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub print: Option<Fingerprint>,
    pub service: Option<Box<str>>,
    pub text: Option<Box<str>>,
    limit: u32,
}

impl IssueFilter {
    pub fn over(window: Window) -> Self {
        Self {
            window,
            project: None,
            print: None,
            service: None,
            text: None,
            limit: DEFAULT_LIMIT,
        }
    }

    #[must_use]
    pub fn at_most(mut self, asked: u32) -> Self {
        self.limit = asked.clamp(1, MAX_LIMIT);
        self
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }
}
