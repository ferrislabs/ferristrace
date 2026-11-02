use uuid::Uuid;

use super::entities::{Rotation, Schedule};
use super::value_objects::{
    Day, Handoff, RotationLength, ScheduleId, ScheduleName, TimeOfDay, Timezone, Weekday,
};
use crate::access::key_ring_tests::{MINE, THEIRS, an_org};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::organisation::{OrgRole, TeamId};
use crate::testing::{Bench, Crew, OncallLedger};

pub(crate) const ME: u128 = 9;
pub(crate) const ADA: u128 = 11;
pub(crate) const BOB: u128 = 12;
pub(crate) const BOSS: u128 = 13;
pub(crate) const STRANGER: u128 = 14;
pub(crate) const TEAM: u128 = 300;
pub(crate) const SCHEDULE: u128 = 400;
pub(crate) const START: u64 = 1_793_613_600;

pub(crate) fn person(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

pub(crate) fn team() -> TeamId {
    TeamId::new(Uuid::from_u128(TEAM))
}

pub(crate) fn at(seconds: u64) -> UnixSeconds {
    UnixSeconds::new(START + seconds)
}

pub(crate) fn ms(seconds: u64) -> u64 {
    (START + seconds) * 1_000
}

pub(crate) fn weekly() -> Schedule {
    Schedule {
        id: ScheduleId::new(Uuid::from_u128(SCHEDULE)),
        org: an_org(MINE),
        team: team(),
        name: ScheduleName::try_from("Primary").expect("a name"),
        timezone: Timezone::utc(),
        rotation: Rotation::new(vec![person(ME), person(ADA), person(BOB)]).expect("a rotation"),
        handoff: Handoff {
            weekday: Weekday::Monday,
            time: TimeOfDay::try_from("09:00").expect("a time"),
        },
        length: RotationLength::try_from(7).expect("a length"),
        starts_on: Day::try_from("2026-11-02").expect("a day"),
    }
}

pub(crate) fn crew() -> Crew {
    let crew = Crew::default();
    crew.seat(an_org(MINE), person(ME), "me@example.com");
    crew.seat(an_org(MINE), person(ADA), "ada@example.com");
    crew.seat(an_org(MINE), person(BOB), "bob@example.com");
    crew.seat_as(
        an_org(MINE),
        person(BOSS),
        "boss@example.com",
        OrgRole::Admin,
    );
    crew.seat(an_org(THEIRS), person(STRANGER), "stranger@example.com");
    crew
}

pub(crate) fn bench() -> Bench {
    let bench = Bench::default();
    bench.team(
        an_org(MINE),
        team(),
        "Payments",
        &[person(ME), person(ADA), person(BOB)],
    );
    bench
}

pub(crate) fn oncall() -> OncallLedger {
    let ledger = OncallLedger::default();
    ledger.put(weekly());
    for (seed, email) in [
        (ME, "me@example.com"),
        (ADA, "ada@example.com"),
        (BOB, "bob@example.com"),
        (BOSS, "boss@example.com"),
    ] {
        ledger.address(person(seed), email);
    }
    ledger
}
