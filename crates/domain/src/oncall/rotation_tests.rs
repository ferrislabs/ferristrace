use uuid::Uuid;

use super::entities::{Override, Rotation, Schedule, Shift};
use super::rotation::{on_call, shift};
use super::value_objects::{
    Day, Handoff, OverrideId, RotationLength, ScheduleId, ScheduleName, TimeOfDay, Timezone,
    Weekday,
};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::organisation::TeamId;
use crate::telemetry::TenantId;

fn person(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

const ADA: u128 = 1;
const BOB: u128 = 2;
const CAT: u128 = 3;
const DAN: u128 = 4;

fn at(stamp: &str) -> UnixSeconds {
    let instant: jiff::Timestamp = stamp.parse().expect("an instant");
    UnixSeconds::new(u64::try_from(instant.as_second()).expect("after the epoch"))
}

fn millis(stamp: &str) -> u64 {
    at(stamp).get() * 1_000
}

fn schedule(zone: &str, weekday: Weekday, time: &str, days: u64, starts: &str) -> Schedule {
    Schedule {
        id: ScheduleId::new(Uuid::from_u128(100)),
        org: TenantId::new(Uuid::from_u128(200)),
        team: TeamId::new(Uuid::from_u128(300)),
        name: ScheduleName::try_from("Primary").expect("a name"),
        timezone: Timezone::try_from(zone).expect("a timezone"),
        rotation: Rotation::new(vec![person(ADA), person(BOB), person(CAT)]).expect("a rotation"),
        handoff: Handoff {
            weekday,
            time: TimeOfDay::try_from(time).expect("a time"),
        },
        length: RotationLength::try_from(days).expect("a length"),
        starts_on: Day::try_from(starts).expect("a day"),
    }
}

fn weekly() -> Schedule {
    schedule("UTC", Weekday::Monday, "09:00", 7, "2026-11-02")
}

#[test]
fn nobody_is_on_call_before_the_first_handoff() {
    assert_eq!(on_call(&weekly(), &[], at("2026-11-02T08:59:59Z")), None);
}

#[test]
fn the_first_person_takes_the_first_slot_at_the_handoff() {
    assert_eq!(
        on_call(&weekly(), &[], at("2026-11-02T09:00:00Z")),
        Some(person(ADA))
    );
    assert_eq!(
        on_call(&weekly(), &[], at("2026-11-09T08:59:59Z")),
        Some(person(ADA))
    );
}

#[test]
fn each_handoff_passes_the_pager_to_the_next_person_and_wraps() {
    let held = weekly();

    assert_eq!(
        on_call(&held, &[], at("2026-11-09T09:00:00Z")),
        Some(person(BOB))
    );
    assert_eq!(
        on_call(&held, &[], at("2026-11-16T09:00:00Z")),
        Some(person(CAT))
    );
    assert_eq!(
        on_call(&held, &[], at("2026-11-23T09:00:00Z")),
        Some(person(ADA))
    );
    assert_eq!(
        on_call(&held, &[], at("2027-03-01T12:00:00Z")),
        Some(person(CAT)),
        "seventeen weeks later the rotation still counts whole weeks"
    );
}

#[test]
fn a_schedule_starting_midweek_waits_for_its_handoff_weekday() {
    let held = schedule("UTC", Weekday::Monday, "09:00", 7, "2026-11-04");

    assert_eq!(on_call(&held, &[], at("2026-11-06T12:00:00Z")), None);
    assert_eq!(
        on_call(&held, &[], at("2026-11-09T09:00:00Z")),
        Some(person(ADA))
    );
}

#[test]
fn a_daily_rotation_hands_off_every_day() {
    let held = schedule("UTC", Weekday::Tuesday, "18:30", 1, "2026-11-03");

    assert_eq!(
        on_call(&held, &[], at("2026-11-03T18:30:00Z")),
        Some(person(ADA))
    );
    assert_eq!(
        on_call(&held, &[], at("2026-11-04T18:29:00Z")),
        Some(person(ADA))
    );
    assert_eq!(
        on_call(&held, &[], at("2026-11-04T18:30:00Z")),
        Some(person(BOB))
    );
    assert_eq!(
        on_call(&held, &[], at("2026-11-06T08:00:00Z")),
        Some(person(CAT))
    );
}

#[test]
fn a_shift_lasts_until_the_next_handoff() {
    assert_eq!(
        shift(&weekly(), &[], at("2026-11-10T00:00:00Z")),
        Some(Shift {
            account: person(BOB),
            until_ms: Some(millis("2026-11-16T09:00:00Z")),
        })
    );
}

fn cover(account: u128, from: &str, to: &str) -> Override {
    Override {
        id: OverrideId::fresh(),
        schedule: ScheduleId::new(Uuid::from_u128(100)),
        account: person(account),
        from_ms: millis(from),
        to_ms: millis(to),
    }
}

#[test]
fn an_override_wins_over_the_rotation_while_it_lasts() {
    let overrides = [cover(DAN, "2026-11-10T00:00:00Z", "2026-11-11T00:00:00Z")];
    let held = weekly();

    assert_eq!(
        shift(&held, &overrides, at("2026-11-10T12:00:00Z")),
        Some(Shift {
            account: person(DAN),
            until_ms: Some(millis("2026-11-11T00:00:00Z")),
        })
    );
    assert_eq!(
        on_call(&held, &overrides, at("2026-11-11T00:00:00Z")),
        Some(person(BOB)),
        "the override ends where it says"
    );
    assert_eq!(
        shift(&held, &overrides, at("2026-11-09T12:00:00Z")),
        Some(Shift {
            account: person(BOB),
            until_ms: Some(millis("2026-11-10T00:00:00Z")),
        }),
        "the shift before an override ends when the override begins"
    );
}

#[test]
fn an_override_covers_people_before_the_rotation_starts() {
    let overrides = [cover(DAN, "2026-11-01T00:00:00Z", "2026-11-02T10:00:00Z")];

    assert_eq!(
        on_call(&weekly(), &overrides, at("2026-11-01T12:00:00Z")),
        Some(person(DAN))
    );
}

#[test]
fn the_latest_override_wins_where_two_overlap() {
    let overrides = [
        cover(DAN, "2026-11-10T00:00:00Z", "2026-11-12T00:00:00Z"),
        cover(CAT, "2026-11-11T00:00:00Z", "2026-11-11T06:00:00Z"),
    ];

    assert_eq!(
        on_call(&weekly(), &overrides, at("2026-11-11T03:00:00Z")),
        Some(person(CAT))
    );
}

#[test]
fn an_empty_rotation_has_nobody_on_call() {
    let mut held = weekly();
    held.rotation = Rotation::new(Vec::new()).expect("an empty rotation");

    assert_eq!(on_call(&held, &[], at("2026-11-20T09:00:00Z")), None);
}

#[test]
fn a_handoff_keeps_its_local_time_across_daylight_saving() {
    let held = schedule("Europe/Paris", Weekday::Saturday, "09:00", 1, "2026-10-24");

    assert_eq!(
        on_call(&held, &[], at("2026-10-24T07:00:00Z")),
        Some(person(ADA)),
        "09:00 in Paris summer time is 07:00 UTC"
    );
    assert_eq!(
        on_call(&held, &[], at("2026-10-25T07:30:00Z")),
        Some(person(ADA)),
        "after the clocks go back, 07:30 UTC is 08:30 in Paris, before the handoff"
    );
    assert_eq!(
        on_call(&held, &[], at("2026-10-25T08:00:00Z")),
        Some(person(BOB)),
        "09:00 in Paris winter time is 08:00 UTC"
    );
    assert_eq!(
        shift(&held, &[], at("2026-10-24T12:00:00Z")).and_then(|shift| shift.until_ms),
        Some(millis("2026-10-25T08:00:00Z")),
        "that day lasts 25 hours"
    );
}

#[test]
fn a_handoff_in_a_skipped_hour_lands_after_the_gap() {
    let held = schedule("Europe/Paris", Weekday::Saturday, "02:30", 1, "2026-03-28");

    assert_eq!(
        on_call(&held, &[], at("2026-03-29T00:59:00Z")),
        Some(person(ADA))
    );
    assert_eq!(
        on_call(&held, &[], at("2026-03-29T01:30:00Z")),
        Some(person(BOB)),
        "02:30 does not exist that night, so the handoff happens at 03:30 summer time"
    );
}
