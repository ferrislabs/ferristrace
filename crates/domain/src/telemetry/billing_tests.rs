use super::billing::{BillingHour, BillingMonth};
use crate::clock::UnixSeconds;

#[test]
fn a_known_instant_lands_in_its_calendar_month() {
    let instant = UnixSeconds::new(1_700_000_000);

    assert_eq!(BillingMonth::containing(instant).key(), "202311");
}

#[test]
fn the_epoch_is_the_first_month() {
    assert_eq!(
        BillingMonth::containing(UnixSeconds::new(0)).key(),
        "197001"
    );
}

#[test]
fn a_december_instant_does_not_roll_into_the_next_year() {
    let last_second_of_2026 = UnixSeconds::new(1_798_761_599);

    assert_eq!(
        BillingMonth::containing(last_second_of_2026).key(),
        "202612"
    );
}

#[test]
fn an_instant_beyond_the_representable_range_falls_back_to_the_epoch() {
    assert_eq!(
        BillingMonth::containing(UnixSeconds::new(u64::MAX)).key(),
        "197001"
    );
}

#[test]
fn a_month_starts_at_midnight_utc_on_its_first_day() {
    let november = BillingMonth::containing(UnixSeconds::new(1_700_000_000));

    assert_eq!(november.start_nanos(), 1_698_796_800 * 1_000_000_000);
}

#[test]
fn the_start_of_a_month_belongs_to_that_month() {
    let month = BillingMonth::containing(UnixSeconds::new(1_700_000_000));
    let start = u64::try_from(month.start_nanos() / 1_000_000_000).expect("after the epoch");

    assert_eq!(BillingMonth::containing(UnixSeconds::new(start)), month);
}

#[test]
fn an_hour_is_named_down_to_the_hour() {
    let hour = BillingHour::containing(UnixSeconds::new(1_700_000_000));

    assert_eq!(hour.key(), "2023111422");
}

#[test]
fn an_instant_inside_an_hour_names_that_hour() {
    let start = 1_699_999_200;
    let early = BillingHour::containing(UnixSeconds::new(start));
    let late = BillingHour::containing(UnixSeconds::new(start + 3599));

    assert_eq!(early, late);
}

#[test]
fn the_next_second_belongs_to_the_next_hour() {
    let hour = BillingHour::containing(UnixSeconds::new(1_700_000_000));
    let next = BillingHour::containing(UnixSeconds::new(1_700_000_000 + 3600));

    assert_ne!(hour, next);
    assert_eq!(next.previous(), hour);
}

#[test]
fn an_hour_starts_on_a_whole_hour() {
    let hour = BillingHour::containing(UnixSeconds::new(1_700_000_000));

    assert_eq!(hour.start_nanos() % (3600 * 1_000_000_000), 0);
}
