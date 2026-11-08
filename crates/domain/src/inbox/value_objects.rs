use core::fmt;
use std::collections::BTreeMap;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InboxItemKind {
    Issue,
    ServiceSilent,
    StatusPage,
    ApiKeyExpiring,
    Usage,
    Membership,
}

pub const KINDS: [InboxItemKind; 6] = [
    InboxItemKind::Issue,
    InboxItemKind::ServiceSilent,
    InboxItemKind::StatusPage,
    InboxItemKind::ApiKeyExpiring,
    InboxItemKind::Usage,
    InboxItemKind::Membership,
];

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

    /// What a freshly-created account is subscribed to before it ever touches
    /// its notification preferences. Status page and membership start
    /// in-app-only; everything else starts on both channels.
    pub fn default_channels(self) -> Channels {
        match self {
            Self::StatusPage | Self::Membership => Channels::in_app_only(),
            Self::Issue | Self::ServiceSilent | Self::ApiKeyExpiring | Self::Usage => {
                Channels::both()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channels {
    pub in_app: bool,
    pub email: bool,
}

impl Channels {
    pub const fn both() -> Self {
        Self {
            in_app: true,
            email: true,
        }
    }

    pub const fn in_app_only() -> Self {
        Self {
            in_app: true,
            email: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPreferences {
    pub email_enabled: bool,
    pub weekly_digest: bool,
    pub channels: BTreeMap<InboxItemKind, Channels>,
}

impl NotificationPreferences {
    pub fn channels_for(&self, kind: InboxItemKind) -> Channels {
        self.channels
            .get(&kind)
            .copied()
            .unwrap_or_else(|| kind.default_channels())
    }
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            email_enabled: true,
            weekly_digest: false,
            channels: KINDS
                .into_iter()
                .map(|kind| (kind, kind.default_channels()))
                .collect(),
        }
    }
}
