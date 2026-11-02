use super::value_objects::IngestKey;

#[test]
fn a_key_splits_into_a_prefix_and_a_secret() {
    let key = IngestKey::from_plaintext("ft_live_abc123.the-secret");

    assert_eq!(key.split(), Some(("ft_live_abc123", "the-secret")));
}

#[test]
fn a_key_missing_either_half_splits_into_nothing() {
    for candidate in ["ft_live_abc123", "ft_live_abc123.", ".the-secret", "."] {
        assert_eq!(
            IngestKey::from_plaintext(candidate).split(),
            None,
            "{candidate} should not split"
        );
    }
}

#[test]
fn only_the_first_separator_divides_the_key() {
    let key = IngestKey::from_plaintext("prefix.secret.with.dots");

    assert_eq!(key.split(), Some(("prefix", "secret.with.dots")));
}

#[test]
fn a_key_never_renders_itself() {
    let key = IngestKey::from_plaintext("ft_live_abc123.the-secret");

    assert_eq!(format!("{key:?}"), "IngestKey(redacted)");
}
