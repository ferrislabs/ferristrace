use jiff::civil::{Date, Time};
use jiff::{Span, Timestamp};

use super::entities::{Override, Schedule, Shift};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;

const MILLIS_PER_SECOND: u64 = 1_000;
const SECONDS_PER_DAY: i64 = 86_400;
const DAYS_PER_WEEK: i8 = 7;

pub fn on_call(schedule: &Schedule, overrides: &[Override], at: UnixSeconds) -> Option<AccountId> {
    shift(schedule, overrides, at).map(|shift| shift.account)
}

pub fn shift(schedule: &Schedule, overrides: &[Override], at: UnixSeconds) -> Option<Shift> {
    let at_ms = at.get().saturating_mul(MILLIS_PER_SECOND);
    if let Some(cover) = overrides
        .iter()
        .filter(|cover| cover.schedule == schedule.id && cover.covers(at_ms))
        .max_by_key(|cover| cover.from_ms)
    {
        return Some(Shift {
            account: cover.account,
            until_ms: Some(cover.to_ms),
        });
    }
    let people = schedule.rotation.people();
    let (slot, next_ms) = slot_at(schedule, at_ms)?;
    let index = usize::try_from(slot % u64::try_from(people.len()).ok()?).ok()?;
    let account = *people.get(index)?;
    let next_cover = overrides
        .iter()
        .filter(|cover| cover.schedule == schedule.id && cover.from_ms > at_ms)
        .map(|cover| cover.from_ms)
        .min();
    let until_ms = match (next_ms, next_cover) {
        (Some(handoff), Some(cover)) => Some(handoff.min(cover)),
        (handoff, cover) => handoff.or(cover),
    };
    Some(Shift { account, until_ms })
}

pub(super) fn handoff_ms(schedule: &Schedule, slot: i64) -> Option<u64> {
    let length = i64::from(schedule.length.days());
    let date = first_handoff(schedule)?
        .checked_add(Span::new().try_days(slot.checked_mul(length)?).ok()?)
        .ok()?;
    let time = Time::new(
        i8::try_from(schedule.handoff.time.hour()).ok()?,
        i8::try_from(schedule.handoff.time.minute()).ok()?,
        0,
        0,
    )
    .ok()?;
    let instant = schedule
        .timezone
        .zone()
        .to_timestamp(date.to_datetime(time))
        .ok()?;
    u64::try_from(instant.as_millisecond()).ok()
}

fn first_handoff(schedule: &Schedule) -> Option<Date> {
    let start = schedule.starts_on.date();
    let from = start.weekday().to_monday_zero_offset();
    let to = i8::try_from(schedule.handoff.weekday.days_from_monday()).ok()?;
    let ahead = (to - from).rem_euclid(DAYS_PER_WEEK);
    start.checked_add(Span::new().days(ahead)).ok()
}

fn slot_at(schedule: &Schedule, at_ms: u64) -> Option<(u64, Option<u64>)> {
    if schedule.rotation.is_empty() {
        return None;
    }
    let first = first_handoff(schedule)?;
    let instant = Timestamp::from_millisecond(i64::try_from(at_ms).ok()?).ok()?;
    let local = schedule.timezone.zone().to_datetime(instant).date();
    let days = local
        .duration_since(first)
        .as_secs()
        .div_euclid(SECONDS_PER_DAY);
    let mut slot = days.div_euclid(i64::from(schedule.length.days()));
    if at_ms < handoff_ms(schedule, slot)? {
        slot -= 1;
    }
    let held = u64::try_from(slot).ok()?;
    Some((held, handoff_ms(schedule, slot + 1)))
}
