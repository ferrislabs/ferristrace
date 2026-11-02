use super::value_objects::SessionLifetime;
use crate::clock::UnixSeconds;

const CREATED: u64 = 1_700_000_000;

fn lifetime() -> SessionLifetime {
    SessionLifetime {
        idle_seconds: 100,
        hard_seconds: 1000,
    }
}

#[test]
fn the_idle_window_slides_while_it_stays_inside_the_hard_life() {
    let expiry = lifetime().expiry_from(UnixSeconds::new(CREATED), UnixSeconds::new(CREATED + 500));

    assert_eq!(expiry, UnixSeconds::new(CREATED + 600));
}

#[test]
fn the_hard_life_caps_the_sliding_window() {
    let expiry = lifetime().expiry_from(UnixSeconds::new(CREATED), UnixSeconds::new(CREATED + 950));

    assert_eq!(
        expiry,
        UnixSeconds::new(CREATED + 1000),
        "sliding must not reach past the hard life"
    );
}

#[test]
fn a_fresh_session_expires_after_one_idle_window() {
    let created = UnixSeconds::new(CREATED);

    assert_eq!(
        lifetime().expiry_from(created, created),
        UnixSeconds::new(CREATED + 100)
    );
}

#[test]
fn the_default_lifetime_is_a_working_day_inside_a_month() {
    let default = SessionLifetime::default();

    assert_eq!(default.idle_seconds, 8 * 60 * 60);
    assert_eq!(default.hard_seconds, 30 * 24 * 60 * 60);
}
