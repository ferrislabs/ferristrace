use core::fmt;

use uuid::Uuid;

pub const RETENTION_DAYS: u64 = 90;
pub const PAGE_LIMIT: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InboxItemId(Uuid);

impl InboxItemId {
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

impl fmt::Display for InboxItemId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// What raised this item. Each variant is a future producer's target; none of
/// them write one yet (see docs/chantiers/account-preferences.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxItemKind {
    Issue,
    ServiceSilent,
    StatusPage,
    ApiKeyExpiring,
    Usage,
    Membership,
}

impl InboxItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::ServiceSilent => "service_silent",
            Self::StatusPage => "status_page",
            Self::ApiKeyExpiring => "api_key_expiring",
            Self::Usage => "usage",
            Self::Membership => "membership",
        }
    }

    pub fn parse(spelling: &str) -> Option<Self> {
        match spelling {
            "issue" => Some(Self::Issue),
            "service_silent" => Some(Self::ServiceSilent),
            "status_page" => Some(Self::StatusPage),
            "api_key_expiring" => Some(Self::ApiKeyExpiring),
            "usage" => Some(Self::Usage),
            "membership" => Some(Self::Membership),
            _ => None,
        }
    }
}
