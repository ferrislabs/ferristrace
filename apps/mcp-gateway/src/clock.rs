use jiff::Timestamp;

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Timestamp;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixedClock(Timestamp);

impl FixedClock {
    pub fn at(instant: Timestamp) -> Self {
        Self(instant)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTime {
    pub weekday: i8,
    pub hour: i8,
    pub minute: i8,
    pub timezone: String,
    pub iso_utc: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0} is not a time zone")]
pub struct UnknownZone(pub String);

pub fn local_time(now: Timestamp, timezone: &str) -> Result<LocalTime, UnknownZone> {
    let zone = jiff::tz::TimeZone::get(timezone).map_err(|_| UnknownZone(timezone.to_owned()))?;
    let whole =
        Timestamp::from_second(now.as_second()).map_err(|_| UnknownZone(timezone.to_owned()))?;
    let local = whole.to_zoned(zone);
    Ok(LocalTime {
        weekday: local.weekday().to_monday_one_offset(),
        hour: local.hour(),
        minute: local.minute(),
        timezone: timezone.to_owned(),
        iso_utc: whole.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn a_fixed_clock_does_not_move() {
        let clock = FixedClock::at(at("2026-11-03T10:00:00Z"));
        assert_eq!(clock.now(), clock.now());
        assert_eq!(clock.now(), at("2026-11-03T10:00:00Z"));
    }

    #[test]
    fn the_system_clock_is_past_2026() {
        assert!(SystemClock.now() > at("2026-01-01T00:00:00Z"));
    }

    #[test]
    fn utc_is_read_as_it_is() {
        let local = local_time(at("2026-11-03T10:30:45Z"), "UTC").unwrap();
        assert_eq!(
            local,
            LocalTime {
                weekday: 2,
                hour: 10,
                minute: 30,
                timezone: "UTC".to_owned(),
                iso_utc: "2026-11-03T10:30:45Z".to_owned(),
            }
        );
    }

    #[test]
    fn monday_is_one_and_sunday_is_seven() {
        assert_eq!(
            local_time(at("2026-11-02T12:00:00Z"), "UTC")
                .unwrap()
                .weekday,
            1
        );
        assert_eq!(
            local_time(at("2026-11-08T12:00:00Z"), "UTC")
                .unwrap()
                .weekday,
            7
        );
    }

    #[test]
    fn the_weekday_and_hour_are_the_organisations_not_utc() {
        let local = local_time(at("2026-11-02T23:30:00Z"), "Asia/Tokyo").unwrap();
        assert_eq!((local.weekday, local.hour, local.minute), (2, 8, 30));
        assert_eq!(local.iso_utc, "2026-11-02T23:30:00Z");
        let behind = local_time(at("2026-11-03T02:15:00Z"), "America/Los_Angeles").unwrap();
        assert_eq!((behind.weekday, behind.hour, behind.minute), (1, 18, 15));
    }

    #[test]
    fn a_half_hour_zone_keeps_its_minutes() {
        let local = local_time(at("2026-11-03T10:00:00Z"), "Asia/Kolkata").unwrap();
        assert_eq!((local.hour, local.minute), (15, 30));
    }

    #[test]
    fn paris_moves_by_an_hour_across_the_spring_transition() {
        let before = local_time(at("2026-03-28T08:00:00Z"), "Europe/Paris").unwrap();
        let after = local_time(at("2026-03-30T08:00:00Z"), "Europe/Paris").unwrap();
        assert_eq!((before.weekday, before.hour), (6, 9));
        assert_eq!((after.weekday, after.hour), (1, 10));
    }

    #[test]
    fn paris_moves_by_an_hour_across_the_autumn_transition() {
        let before = local_time(at("2026-10-24T07:00:00Z"), "Europe/Paris").unwrap();
        let after = local_time(at("2026-10-26T07:00:00Z"), "Europe/Paris").unwrap();
        assert_eq!(before.hour, 9);
        assert_eq!(after.hour, 8);
    }

    #[test]
    fn the_instant_inside_the_gap_and_the_fold_is_read_correctly() {
        let gap_edge = local_time(at("2026-03-29T00:59:59Z"), "Europe/Paris").unwrap();
        let past_gap = local_time(at("2026-03-29T01:00:00Z"), "Europe/Paris").unwrap();
        assert_eq!((gap_edge.hour, gap_edge.minute), (1, 59));
        assert_eq!((past_gap.hour, past_gap.minute), (3, 0));
        let first = local_time(at("2026-10-25T00:30:00Z"), "Europe/Paris").unwrap();
        let second = local_time(at("2026-10-25T01:30:00Z"), "Europe/Paris").unwrap();
        assert_eq!((first.hour, first.minute), (2, 30));
        assert_eq!((second.hour, second.minute), (2, 30));
    }

    #[test]
    fn fractional_seconds_are_not_sent() {
        let local = local_time(at("2026-11-03T10:00:00.987Z"), "UTC").unwrap();
        assert_eq!(local.iso_utc, "2026-11-03T10:00:00Z");
    }

    #[test]
    fn an_unknown_zone_is_refused() {
        assert_eq!(
            local_time(at("2026-11-03T10:00:00Z"), "Mars/Olympus"),
            Err(UnknownZone("Mars/Olympus".to_owned()))
        );
        assert!(local_time(at("2026-11-03T10:00:00Z"), "").is_err());
    }
}
