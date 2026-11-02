use super::errors::BadStatusPage;
use super::value_objects::{
    Accent, ComponentName, ComponentStatus, Intro, MAX_INTRO_BYTES, MAX_SLUG_BYTES, MIN_SLUG_BYTES,
    Mode, PageName, ServiceName, Slug,
};

#[test]
fn a_slug_is_lowercase_letters_digits_and_hyphens_within_bounds() {
    for good in [
        "abc",
        "acme-prod",
        "a1-b2-c3",
        "---",
        &"x".repeat(MAX_SLUG_BYTES),
    ] {
        assert_eq!(
            Slug::try_from(good).map(|slug| slug.as_str().to_owned()),
            Ok(good.to_owned()),
            "{good}"
        );
    }
}

#[test]
fn a_slug_outside_the_alphabet_or_the_bounds_is_refused() {
    let long = "x".repeat(MAX_SLUG_BYTES + 1);
    let short = "x".repeat(MIN_SLUG_BYTES - 1);
    for bad in [
        "Acme",
        "acme_prod",
        "acme prod",
        "acmé",
        "a.b",
        "",
        &short,
        &long,
        " acme",
    ] {
        assert_eq!(Slug::try_from(bad), Err(BadStatusPage::Slug), "{bad}");
    }
}

#[test]
fn the_public_routes_are_not_slugs() {
    for reserved in ["confirm", "unsubscribe"] {
        assert_eq!(Slug::try_from(reserved), Err(BadStatusPage::Slug));
    }
}

#[test]
fn names_are_trimmed_and_bounded() {
    assert_eq!(
        PageName::try_from("  Acme  ").map(|name| name.as_str().to_owned()),
        Ok("Acme".to_owned())
    );
    assert!(PageName::try_from("   ").is_err());
    assert!(PageName::try_from("x".repeat(101).as_str()).is_err());
    assert!(ComponentName::try_from("Checkout").is_ok());
    assert!(ComponentName::try_from("").is_err());
    assert!(ServiceName::try_from("checkout-api").is_ok());
    assert!(ServiceName::try_from(" ").is_err());
}

#[test]
fn an_intro_holds_up_to_its_bound() {
    assert!(Intro::try_from("x".repeat(MAX_INTRO_BYTES).as_str()).is_ok());
    assert!(Intro::try_from("x".repeat(MAX_INTRO_BYTES + 1).as_str()).is_err());
    assert!(Intro::try_from("  ").is_err());
}

#[test]
fn themes_and_statuses_read_back_from_their_spelling() {
    for accent in Accent::ALL {
        assert_eq!(Accent::parse(accent.as_str()), Some(*accent));
    }
    for mode in Mode::ALL {
        assert_eq!(Mode::parse(mode.as_str()), Some(*mode));
    }
    for status in ComponentStatus::ALL {
        assert_eq!(ComponentStatus::parse(status.as_str()), Some(*status));
    }
    assert_eq!(Accent::parse("purple"), None);
    assert_eq!(Mode::parse("sepia"), None);
}

#[test]
fn a_worse_status_orders_after_a_better_one() {
    assert!(ComponentStatus::Operational < ComponentStatus::Degraded);
    assert!(ComponentStatus::Degraded < ComponentStatus::Down);
}
