use crate::clock::UnixSeconds;
use time::{Date, Month, OffsetDateTime};

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const SECONDS_PER_HOUR: i64 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BillingMonth {
    year: i32,
    month: u8,
}

impl BillingMonth {
    pub fn containing(instant: UnixSeconds) -> Self {
        let seconds = i64::try_from(instant.get()).unwrap_or(i64::MAX);
        let moment =
            OffsetDateTime::from_unix_timestamp(seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH);
        Self {
            year: moment.year(),
            month: u8::from(moment.month()),
        }
    }

    pub fn start_nanos(self) -> i64 {
        let month = Month::try_from(self.month).unwrap_or(Month::January);
        let first = Date::from_calendar_date(self.year, month, 1)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH.date());
        first.midnight().assume_utc().unix_timestamp() * NANOS_PER_SECOND
    }

    pub fn key(self) -> String {
        format!("{:04}{:02}", self.year, self.month)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BillingHour {
    start: i64,
}

impl BillingHour {
    pub fn containing(instant: UnixSeconds) -> Self {
        let seconds = i64::try_from(instant.get()).unwrap_or(i64::MAX);
        Self {
            start: seconds - seconds.rem_euclid(SECONDS_PER_HOUR),
        }
    }

    pub fn previous(self) -> Self {
        Self {
            start: self.start.saturating_sub(SECONDS_PER_HOUR),
        }
    }

    pub fn start_nanos(self) -> i64 {
        self.start.saturating_mul(NANOS_PER_SECOND)
    }

    pub fn key(self) -> String {
        let moment =
            OffsetDateTime::from_unix_timestamp(self.start).unwrap_or(OffsetDateTime::UNIX_EPOCH);
        format!(
            "{:04}{:02}{:02}{:02}",
            moment.year(),
            u8::from(moment.month()),
            moment.day(),
            moment.hour()
        )
    }
}
