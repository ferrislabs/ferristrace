use super::value_objects::InboxItemKind;

const ALL: [InboxItemKind; 6] = [
    InboxItemKind::Issue,
    InboxItemKind::ServiceSilent,
    InboxItemKind::StatusPage,
    InboxItemKind::ApiKeyExpiring,
    InboxItemKind::Usage,
    InboxItemKind::Membership,
];

#[test]
fn every_spelling_reads_back() {
    for kind in ALL {
        assert_eq!(InboxItemKind::parse(kind.as_str()), Some(kind));
    }
}

#[test]
fn an_unknown_spelling_is_refused() {
    assert_eq!(InboxItemKind::parse("something_else"), None);
}
