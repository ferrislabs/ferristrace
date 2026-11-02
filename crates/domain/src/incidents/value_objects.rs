use core::fmt;

use uuid::Uuid;

use super::errors::BadIncident;
use crate::alerts::AlertSeverity;
use crate::telemetry::TraceId;

pub const MAX_TITLE_BYTES: usize = 200;
pub const MAX_COMMENT_BYTES: usize = 10_000;
pub const MAX_REFERENCE_BYTES: usize = 4_096;
pub const MAX_LABEL_BYTES: usize = 200;

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

identifier!(IncidentId);
identifier!(TimelineId);
identifier!(AttachmentId);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IncidentSeverity {
    Sev1,
    Sev2,
    Sev3,
    Sev4,
}

spelled!(IncidentSeverity {
    Sev1 => "sev1",
    Sev2 => "sev2",
    Sev3 => "sev3",
    Sev4 => "sev4",
});

impl From<AlertSeverity> for IncidentSeverity {
    fn from(severity: AlertSeverity) -> Self {
        match severity {
            AlertSeverity::Critical => Self::Sev2,
            AlertSeverity::Warning => Self::Sev3,
            AlertSeverity::Info => Self::Sev4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IncidentStatus {
    Investigating,
    Identified,
    Monitoring,
    Resolved,
}

spelled!(IncidentStatus {
    Investigating => "investigating",
    Identified => "identified",
    Monitoring => "monitoring",
    Resolved => "resolved",
});

impl IncidentStatus {
    pub fn is_open(self) -> bool {
        self != Self::Resolved
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IncidentScope {
    #[default]
    Open,
    Resolved,
    All,
}

spelled!(IncidentScope {
    Open => "open",
    Resolved => "resolved",
    All => "all",
});

impl IncidentScope {
    pub fn admits(self, status: IncidentStatus) -> bool {
        match self {
            Self::Open => status.is_open(),
            Self::Resolved => !status.is_open(),
            Self::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    Opened,
    Status,
    Severity,
    Owner,
    Comment,
    Alert,
    Attachment,
    Title,
    Public,
}

spelled!(EntryKind {
    Opened => "opened",
    Status => "status",
    Severity => "severity",
    Owner => "owner",
    Comment => "comment",
    Alert => "alert",
    Attachment => "attachment",
    Title => "title",
    Public => "public",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttachmentKind {
    Trace,
    Logs,
    Metric,
}

spelled!(AttachmentKind {
    Trace => "trace",
    Logs => "logs",
    Metric => "metric",
});

fn bounded(chosen: &str, max: usize) -> Option<Box<str>> {
    let kept = chosen.trim();
    (!kept.is_empty() && kept.len() <= max).then(|| kept.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentTitle(Box<str>);

impl IncidentTitle {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for IncidentTitle {
    type Error = BadIncident;

    fn try_from(chosen: &str) -> Result<Self, BadIncident> {
        bounded(chosen, MAX_TITLE_BYTES)
            .map(Self)
            .ok_or(BadIncident::Title {
                max: MAX_TITLE_BYTES,
            })
    }
}

impl fmt::Display for IncidentTitle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentBody(Box<str>);

impl CommentBody {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for CommentBody {
    type Error = BadIncident;

    fn try_from(chosen: &str) -> Result<Self, BadIncident> {
        bounded(chosen, MAX_COMMENT_BYTES)
            .map(Self)
            .ok_or(BadIncident::Comment {
                max: MAX_COMMENT_BYTES,
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentLabel(Box<str>);

impl AttachmentLabel {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for AttachmentLabel {
    type Error = BadIncident;

    fn try_from(chosen: &str) -> Result<Self, BadIncident> {
        bounded(chosen, MAX_LABEL_BYTES)
            .map(Self)
            .ok_or(BadIncident::Label {
                max: MAX_LABEL_BYTES,
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    kind: AttachmentKind,
    text: Box<str>,
}

impl Reference {
    pub fn parse(kind: AttachmentKind, given: &str) -> Result<Self, BadIncident> {
        let kept = given.trim();
        let text: Box<str> = match kind {
            AttachmentKind::Trace => {
                let bytes = hex::decode(kept).map_err(|_| BadIncident::TraceId)?;
                TraceId::try_from(bytes.as_slice()).map_err(|_| BadIncident::TraceId)?;
                kept.to_ascii_lowercase().into()
            }
            AttachmentKind::Logs | AttachmentKind::Metric => bounded(kept, MAX_REFERENCE_BYTES)
                .ok_or(BadIncident::Reference {
                    max: MAX_REFERENCE_BYTES,
                })?,
        };
        Ok(Self { kind, text })
    }

    pub fn kind(&self) -> AttachmentKind {
        self.kind
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}
