use super::value_objects::{InboxItemKind, KINDS, NotificationPreferences};

#[test]
fn every_spelling_reads_back() {
    for kind in KINDS {
        assert_eq!(InboxItemKind::parse(kind.as_str()), Some(kind));
    }
}

#[test]
fn an_unknown_spelling_is_refused() {
    assert_eq!(InboxItemKind::parse("something_else"), None);
}

#[test]
fn the_default_preferences_carry_every_kind_on_its_default_channels() {
    let defaults = NotificationPreferences::default();

    assert!(defaults.email_enabled);
    assert!(!defaults.weekly_digest);
    for kind in KINDS {
        assert_eq!(defaults.channels_for(kind), kind.default_channels());
    }
}

#[test]
fn status_page_and_membership_default_to_in_app_only() {
    assert!(!InboxItemKind::StatusPage.default_channels().email);
    assert!(!InboxItemKind::Membership.default_channels().email);
    assert!(InboxItemKind::Issue.default_channels().email);
}

#[test]
fn a_kind_missing_from_the_map_falls_back_to_its_default() {
    let mostly_empty = NotificationPreferences {
        channels: std::collections::BTreeMap::default(),
        ..NotificationPreferences::default()
    };

    assert_eq!(
        mostly_empty.channels_for(InboxItemKind::Issue),
        InboxItemKind::Issue.default_channels()
    );
}
