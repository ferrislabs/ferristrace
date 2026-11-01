use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct IngestKey(String);

impl IngestKey {
    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }

    pub fn split(&self) -> Option<(&str, &str)> {
        let (prefix, secret) = self.0.split_once('.')?;
        if prefix.is_empty() || secret.is_empty() {
            return None;
        }
        Some((prefix, secret))
    }
}

impl fmt::Debug for IngestKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IngestKey(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use super::IngestKey;

    #[test]
    fn a_key_splits_into_a_prefix_and_a_secret() {
        let key = IngestKey::from_plaintext("ct_live_abc123.the-secret");

        assert_eq!(key.split(), Some(("ct_live_abc123", "the-secret")));
    }

    #[test]
    fn a_key_missing_either_half_splits_into_nothing() {
        for candidate in ["ct_live_abc123", "ct_live_abc123.", ".the-secret", "."] {
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
        let key = IngestKey::from_plaintext("ct_live_abc123.the-secret");

        assert_eq!(format!("{key:?}"), "IngestKey(redacted)");
    }
}
