use core::fmt;
use std::collections::BTreeMap;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::entities::Span;
use super::errors::InvalidId;
use crate::catalogue::ProjectId;

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
        let (prefix, secret) = self.0.split_once('.')?;
        if prefix.is_empty() || secret.is_empty() {
            return None;
        }
        Some((prefix, secret))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    pub tenant: TenantId,
    pub project: Option<ProjectId>,
}

impl Scope {
    pub fn of(tenant: TenantId) -> Self {
        Self {
            tenant,
            project: None,
        }
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
