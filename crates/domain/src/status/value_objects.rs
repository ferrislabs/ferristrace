use core::fmt;

use uuid::Uuid;

use super::errors::BadStatusPage;

pub const MIN_SLUG_BYTES: usize = 3;
pub const MAX_SLUG_BYTES: usize = 48;
pub const MAX_PAGE_NAME_BYTES: usize = 100;
pub const MAX_COMPONENT_NAME_BYTES: usize = 100;
pub const MAX_SERVICE_NAME_BYTES: usize = 200;
pub const MAX_INTRO_BYTES: usize = 2_000;
pub const MAX_COMPONENTS: usize = 50;
pub const DEGRADED_ERROR_PERCENT: u64 = 5;
pub const DOWN_ERROR_PERCENT: u64 = 25;
pub const SERVICE_WINDOW_SECS: u64 = 300;
pub const PUBLIC_WINDOW_DAYS: u64 = 7;
pub const CONFIRM_LIFETIME_SECS: u64 = 7 * 24 * 3_600;
pub const UNSUBSCRIBE_LIFETIME_SECS: u64 = 90 * 24 * 3_600;
pub const RESEND_AFTER_SECS: u64 = 3_600;
pub const ANNOUNCE_TICK_SECS: u64 = 30;
const RESERVED_SLUGS: [&str; 2] = ["confirm", "unsubscribe"];

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

identifier!(StatusPageId);
identifier!(ComponentId);
identifier!(SubscriberId);
identifier!(LetterId);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Accent {
    #[default]
    Indigo,
    Teal,
    Orange,
    Rose,
    Slate,
}

spelled!(Accent {
    Indigo => "indigo",
    Teal => "teal",
    Orange => "orange",
    Rose => "rose",
    Slate => "slate",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    Light,
    Dark,
    #[default]
    Auto,
}

spelled!(Mode {
    Light => "light",
    Dark => "dark",
    Auto => "auto",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Theme {
    pub accent: Accent,
    pub mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum ComponentStatus {
    #[default]
    Operational,
    Degraded,
    Down,
}

spelled!(ComponentStatus {
    Operational => "operational",
    Degraded => "degraded",
    Down => "down",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateKind {
    Opened,
    Status,
    Comment,
}

spelled!(UpdateKind {
    Opened => "opened",
    Status => "status",
    Comment => "comment",
});

impl UpdateKind {
    pub fn public(self) -> &'static str {
        match self {
            Self::Opened | Self::Status => "status",
            Self::Comment => "comment",
        }
    }
}

fn bounded(chosen: &str, max: usize) -> Option<Box<str>> {
    let kept = chosen.trim();
    (!kept.is_empty() && kept.len() <= max).then(|| kept.into())
}

macro_rules! text {
    ($name:ident, $max:expr, $refused:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(Box<str>);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<&str> for $name {
            type Error = BadStatusPage;

            fn try_from(chosen: &str) -> Result<Self, BadStatusPage> {
                bounded(chosen, $max).map(Self).ok_or($refused)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

text!(
    PageName,
    MAX_PAGE_NAME_BYTES,
    BadStatusPage::Name {
        max: MAX_PAGE_NAME_BYTES
    }
);
text!(
    ComponentName,
    MAX_COMPONENT_NAME_BYTES,
    BadStatusPage::ComponentName {
        max: MAX_COMPONENT_NAME_BYTES
    }
);
text!(
    ServiceName,
    MAX_SERVICE_NAME_BYTES,
    BadStatusPage::Service {
        max: MAX_SERVICE_NAME_BYTES
    }
);
text!(
    Intro,
    MAX_INTRO_BYTES,
    BadStatusPage::Intro {
        max: MAX_INTRO_BYTES
    }
);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Slug(Box<str>);

impl Slug {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for Slug {
    type Error = BadStatusPage;

    fn try_from(chosen: &str) -> Result<Self, BadStatusPage> {
        let shaped = (MIN_SLUG_BYTES..=MAX_SLUG_BYTES).contains(&chosen.len())
            && chosen
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !RESERVED_SLUGS.contains(&chosen);
        if shaped {
            Ok(Self(chosen.into()))
        } else {
            Err(BadStatusPage::Slug)
        }
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
