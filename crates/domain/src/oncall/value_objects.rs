use core::fmt;

use jiff::civil::Date;
use jiff::tz::{TimeZone, TimeZoneDatabase};
use uuid::Uuid;

use super::errors::BadSchedule;

pub const MAX_SCHEDULE_NAME_BYTES: usize = 64;
pub const MIN_LENGTH_DAYS: u8 = 1;
pub const MAX_LENGTH_DAYS: u8 = 28;
pub const MAX_ROTATION: usize = 50;
pub const ESCALATE_AFTER_SECS: u64 = 900;
pub const PAGE_TICK_SECS: u64 = 30;
pub const ACK_LIFETIME_SECS: u64 = 86_400;
const MINUTES_PER_DAY: u16 = 1_440;
const MINUTES_PER_HOUR: u16 = 60;

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

identifier!(ScheduleId);
identifier!(OverrideId);
identifier!(PageId);
identifier!(NoticeId);

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
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

spelled!(Weekday {
    Monday => "monday",
    Tuesday => "tuesday",
    Wednesday => "wednesday",
    Thursday => "thursday",
    Friday => "friday",
    Saturday => "saturday",
    Sunday => "sunday",
});

impl Weekday {
    pub fn days_from_monday(self) -> u8 {
        match self {
            Self::Monday => 0,
            Self::Tuesday => 1,
            Self::Wednesday => 2,
            Self::Thursday => 3,
            Self::Friday => 4,
            Self::Saturday => 5,
            Self::Sunday => 6,
        }
    }

    pub fn from_monday(days: u8) -> Option<Self> {
        Self::ALL.get(usize::from(days)).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimeOfDay(u16);

impl TimeOfDay {
    pub fn from_minutes(minutes: u16) -> Result<Self, BadSchedule> {
        if minutes < MINUTES_PER_DAY {
            Ok(Self(minutes))
        } else {
            Err(BadSchedule::Time)
        }
    }

    pub fn minutes(self) -> u16 {
        self.0
    }

    pub fn hour(self) -> u16 {
        self.0 / MINUTES_PER_HOUR
    }

    pub fn minute(self) -> u16 {
        self.0 % MINUTES_PER_HOUR
    }
}

impl TryFrom<&str> for TimeOfDay {
    type Error = BadSchedule;

    fn try_from(text: &str) -> Result<Self, BadSchedule> {
        let (hour, minute) = text.trim().split_once(':').ok_or(BadSchedule::Time)?;
        if hour.is_empty() || hour.len() > 2 || minute.len() != 2 {
            return Err(BadSchedule::Time);
        }
        let hour: u16 = hour.parse().map_err(|_| BadSchedule::Time)?;
        let minute: u16 = minute.parse().map_err(|_| BadSchedule::Time)?;
        if hour >= 24 || minute >= MINUTES_PER_HOUR {
            return Err(BadSchedule::Time);
        }
        Self::from_minutes(hour * MINUTES_PER_HOUR + minute)
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:02}:{:02}", self.hour(), self.minute())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handoff {
    pub weekday: Weekday,
    pub time: TimeOfDay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RotationLength(u8);

impl RotationLength {
    pub fn days(self) -> u8 {
        self.0
    }
}

impl TryFrom<u64> for RotationLength {
    type Error = BadSchedule;

    fn try_from(days: u64) -> Result<Self, BadSchedule> {
        u8::try_from(days)
            .ok()
            .filter(|days| (MIN_LENGTH_DAYS..=MAX_LENGTH_DAYS).contains(days))
            .map(Self)
            .ok_or(BadSchedule::Length {
                min: MIN_LENGTH_DAYS,
                max: MAX_LENGTH_DAYS,
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Day(Date);

impl Day {
    pub fn date(self) -> Date {
        self.0
    }
}

impl TryFrom<&str> for Day {
    type Error = BadSchedule;

    fn try_from(text: &str) -> Result<Self, BadSchedule> {
        let text = text.trim();
        if text.len() != 10 {
            return Err(BadSchedule::Day);
        }
        text.parse::<Date>().map(Self).map_err(|_| BadSchedule::Day)
    }
}

impl fmt::Display for Day {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Timezone {
    name: Box<str>,
    zone: TimeZone,
}

impl Timezone {
    pub fn utc() -> Self {
        Self {
            name: "UTC".into(),
            zone: TimeZone::UTC,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn zone(&self) -> &TimeZone {
        &self.zone
    }
}

impl PartialEq for Timezone {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Timezone {}

impl TryFrom<&str> for Timezone {
    type Error = BadSchedule;

    fn try_from(name: &str) -> Result<Self, BadSchedule> {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_SCHEDULE_NAME_BYTES {
            return Err(BadSchedule::Timezone);
        }
        let zone = TimeZoneDatabase::bundled()
            .get(name)
            .map_err(|_| BadSchedule::Timezone)?;
        Ok(Self {
            name: name.into(),
            zone,
        })
    }
}

impl fmt::Display for Timezone {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleName(Box<str>);

impl ScheduleName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ScheduleName {
    type Error = BadSchedule;

    fn try_from(chosen: &str) -> Result<Self, BadSchedule> {
        let kept = chosen.trim();
        if kept.is_empty() || kept.len() > MAX_SCHEDULE_NAME_BYTES {
            return Err(BadSchedule::Name {
                max: MAX_SCHEDULE_NAME_BYTES,
            });
        }
        Ok(Self(kept.into()))
    }
}

impl fmt::Display for ScheduleName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageState {
    Triggered,
    Acknowledged,
    Escalated,
    Resolved,
}

spelled!(PageState {
    Triggered => "triggered",
    Acknowledged => "acknowledged",
    Escalated => "escalated",
    Resolved => "resolved",
});

impl PageState {
    pub fn is_open(self) -> bool {
        self != Self::Resolved
    }

    pub fn awaits(self) -> bool {
        matches!(self, Self::Triggered | Self::Escalated)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PageScope {
    #[default]
    Open,
    All,
}

spelled!(PageScope {
    Open => "open",
    All => "all",
});

impl PageScope {
    pub fn admits(self, state: PageState) -> bool {
        match self {
            Self::Open => state.is_open(),
            Self::All => true,
        }
    }
}
