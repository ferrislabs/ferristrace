use uuid::Uuid;

use super::entities::Rotation;
use super::errors::BadSchedule;
use super::value_objects::{
    Day, MAX_LENGTH_DAYS, MAX_ROTATION, PageScope, PageState, RotationLength, ScheduleName,
    TimeOfDay, Timezone, Weekday,
};
use crate::identity::AccountId;

#[test]
fn a_time_of_day_is_written_hh_mm() {
    assert_eq!(
        TimeOfDay::try_from("09:05").map(TimeOfDay::minutes),
        Ok(545)
    );
    assert_eq!(
        TimeOfDay::try_from("9:05").map(|time| time.to_string()),
        Ok("09:05".to_owned())
    );
    assert_eq!(
        TimeOfDay::try_from("23:59").map(TimeOfDay::minutes),
        Ok(1_439)
    );
    for bad in ["24:00", "12:60", "12", "12:5", "ab:cd", "", "-1:00"] {
        assert_eq!(TimeOfDay::try_from(bad), Err(BadSchedule::Time), "{bad}");
    }
}

#[test]
fn a_rotation_lasts_one_to_twenty_eight_days() {
    assert!(RotationLength::try_from(1).is_ok());
    assert!(RotationLength::try_from(u64::from(MAX_LENGTH_DAYS)).is_ok());
    for bad in [0, 29, 300] {
        assert!(RotationLength::try_from(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_timezone_is_an_iana_name() {
    assert_eq!(
        Timezone::try_from(" Europe/Paris ").map(|zone| zone.name().to_owned()),
        Ok("Europe/Paris".to_owned())
    );
    assert!(Timezone::try_from("UTC").is_ok());
    assert!(Timezone::try_from("America/New_York").is_ok());
    for bad in ["Mars/Olympus", "", "+02:00"] {
        assert_eq!(Timezone::try_from(bad), Err(BadSchedule::Timezone), "{bad}");
    }
}

#[test]
fn a_day_is_written_yyyy_mm_dd() {
    assert_eq!(
        Day::try_from("2026-11-02").map(|day| day.to_string()),
        Ok("2026-11-02".to_owned())
    );
    for bad in ["2026-02-30", "2026-1-2", "tomorrow", ""] {
        assert_eq!(Day::try_from(bad), Err(BadSchedule::Day), "{bad}");
    }
}

#[test]
fn a_rotation_names_each_person_once() {
    let one = AccountId::new(Uuid::from_u128(1));
    let many: Vec<AccountId> = (0..=MAX_ROTATION)
        .map(|seed| AccountId::new(Uuid::from_u128(seed as u128 + 10)))
        .collect();

    assert!(Rotation::new(vec![one]).is_ok());
    assert!(Rotation::new(Vec::new()).is_ok());
    assert_eq!(
        Rotation::new(vec![one, one]),
        Err(BadSchedule::Rotation { max: MAX_ROTATION })
    );
    assert_eq!(
        Rotation::new(many),
        Err(BadSchedule::Rotation { max: MAX_ROTATION })
    );
}

#[test]
fn a_schedule_name_is_bounded() {
    assert!(ScheduleName::try_from("Primary").is_ok());
    assert!(ScheduleName::try_from("  ").is_err());
    assert!(ScheduleName::try_from(&*"x".repeat(65)).is_err());
}

#[test]
fn weekdays_and_states_spell_themselves() {
    for weekday in Weekday::ALL {
        assert_eq!(Weekday::parse(weekday.as_str()), Some(*weekday));
        assert_eq!(
            Weekday::from_monday(weekday.days_from_monday()),
            Some(*weekday)
        );
    }
    assert_eq!(Weekday::parse("funday"), None);
    for state in PageState::ALL {
        assert_eq!(PageState::parse(state.as_str()), Some(*state));
    }
    assert!(PageScope::Open.admits(PageState::Acknowledged));
    assert!(!PageScope::Open.admits(PageState::Resolved));
    assert!(PageScope::All.admits(PageState::Resolved));
}
