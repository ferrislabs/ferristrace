use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidCredential {
    #[error("an email address needs a local part and a domain")]
    Email,
    #[error("a subject cannot be empty")]
    Subject,
    #[error("an issuer cannot be empty")]
    Issuer,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Email(Box<str>);

impl Email {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for Email {
    type Error = InvalidCredential;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let trimmed = text.trim();
        let Some((local, domain)) = trimmed.split_once('@') else {
            return Err(InvalidCredential::Email);
        };
        let shaped = !local.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && !trimmed.contains(char::is_whitespace);
        if shaped {
            Ok(Self(trimmed.to_ascii_lowercase().into_boxed_str()))
        } else {
            Err(InvalidCredential::Email)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Subject(Box<str>);

impl Subject {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for Subject {
    type Error = InvalidCredential;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(InvalidCredential::Subject);
        }
        Ok(Self(trimmed.into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Issuer {
    Local,
    Federated(Box<str>),
}

impl Issuer {
    pub fn federated(url: &str) -> Result<Self, InvalidCredential> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(InvalidCredential::Issuer);
        }
        Ok(Self::Federated(trimmed.into()))
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Local => "local",
            Self::Federated(url) => url,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Password(String);

impl Password {
    pub fn new(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Password(redacted)")
    }
}

const MIN_PASSWORD_CHARS: usize = 12;
const MAX_PASSWORD_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WeakPassword {
    #[error("a password needs at least {0} characters")]
    TooShort(usize),
    #[error("a password cannot be longer than {0} bytes")]
    TooLong(usize),
    #[error("a password cannot be only spaces")]
    Blank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordPolicy {
    pub min_chars: usize,
    pub max_bytes: usize,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_chars: MIN_PASSWORD_CHARS,
            max_bytes: MAX_PASSWORD_BYTES,
        }
    }
}

impl PasswordPolicy {
    pub fn admits(self, password: &Password) -> Result<(), WeakPassword> {
        let chosen = password.exposed();
        if chosen.len() > self.max_bytes {
            return Err(WeakPassword::TooLong(self.max_bytes));
        }
        if chosen.chars().count() < self.min_chars {
            return Err(WeakPassword::TooShort(self.min_chars));
        }
        if chosen.trim().is_empty() {
            return Err(WeakPassword::Blank);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Email, InvalidCredential, Issuer, Password, PasswordPolicy, Subject, WeakPassword,
    };

    #[test]
    fn an_email_is_lowercased_and_trimmed() {
        let email = Email::try_from("  Person@Example.COM ").expect("a shaped address");

        assert_eq!(email.as_str(), "person@example.com");
    }

    #[test]
    fn a_misshapen_email_is_refused() {
        for candidate in [
            "person",
            "@example.com",
            "person@example",
            "person@.com",
            "person@example.",
            "per son@example.com",
        ] {
            assert_eq!(
                Email::try_from(candidate),
                Err(InvalidCredential::Email),
                "{candidate} should be refused"
            );
        }
    }

    #[test]
    fn a_subject_cannot_be_blank() {
        assert_eq!(Subject::try_from("   "), Err(InvalidCredential::Subject));
        assert_eq!(
            Subject::try_from(" sub-123 ").map(|s| s.as_str().to_owned()),
            Ok("sub-123".to_owned())
        );
    }

    #[test]
    fn the_local_issuer_is_not_a_url() {
        assert!(Issuer::Local.is_local());
        assert_eq!(Issuer::Local.as_str(), "local");

        let federated = Issuer::federated("https://id.example").expect("a url");
        assert!(!federated.is_local());
        assert_eq!(federated.as_str(), "https://id.example");
        assert_eq!(Issuer::federated("  "), Err(InvalidCredential::Issuer));
    }

    #[test]
    fn a_password_never_renders_itself() {
        let password = Password::new("correct horse battery");

        assert_eq!(format!("{password:?}"), "Password(redacted)");
        assert_eq!(password.exposed(), "correct horse battery");
    }

    #[test]
    fn a_password_shorter_than_the_floor_is_refused() {
        let policy = PasswordPolicy::default();

        assert_eq!(
            policy.admits(&Password::new("short")),
            Err(WeakPassword::TooShort(12))
        );
        assert_eq!(
            policy.admits(&Password::new("elevenchars")),
            Err(WeakPassword::TooShort(12))
        );
        assert_eq!(policy.admits(&Password::new("twelvechars!")), Ok(()));
    }

    #[test]
    fn a_password_long_enough_to_cost_us_is_refused() {
        let policy = PasswordPolicy::default();

        assert_eq!(policy.admits(&Password::new(&"a".repeat(128))), Ok(()));
        assert_eq!(
            policy.admits(&Password::new(&"a".repeat(129))),
            Err(WeakPassword::TooLong(128))
        );
    }

    #[test]
    fn a_password_of_spaces_is_refused_however_long_it_is() {
        assert_eq!(
            PasswordPolicy::default().admits(&Password::new(&" ".repeat(40))),
            Err(WeakPassword::Blank)
        );
    }

    #[test]
    fn nothing_but_length_is_asked_of_a_password() {
        let policy = PasswordPolicy::default();

        for chosen in [
            "correct horse battery staple",
            "aaaaaaaaaaaaaaaa",
            "il pleut sur la ville",
        ] {
            assert_eq!(
                policy.admits(&Password::new(chosen)),
                Ok(()),
                "{chosen} should be admitted"
            );
        }
    }

    #[test]
    fn the_floor_counts_characters_and_the_ceiling_counts_bytes() {
        let policy = PasswordPolicy::default();
        let four_glyphs = "🜛🜛🜛🜛";

        assert_eq!(four_glyphs.len(), 16, "four glyphs, sixteen bytes");
        assert_eq!(
            policy.admits(&Password::new(four_glyphs)),
            Err(WeakPassword::TooShort(12)),
            "sixteen bytes must not pass for twelve characters"
        );
        assert_eq!(policy.admits(&Password::new(&"🜛".repeat(12))), Ok(()));
        assert_eq!(
            policy.admits(&Password::new(&"🜛".repeat(33))),
            Err(WeakPassword::TooLong(128)),
            "the ceiling is what we pay to hash, so it is counted in bytes"
        );
    }

    #[test]
    fn a_policy_can_be_loosened_or_tightened() {
        let strict = PasswordPolicy {
            min_chars: 20,
            max_bytes: 64,
        };

        assert_eq!(
            strict.admits(&Password::new("twelvechars!")),
            Err(WeakPassword::TooShort(20))
        );
        assert_eq!(
            strict.admits(&Password::new(&"a".repeat(65))),
            Err(WeakPassword::TooLong(64))
        );
    }

    mod proptests {
        use proptest::prelude::*;

        use super::super::{MAX_PASSWORD_BYTES, MIN_PASSWORD_CHARS};
        use super::{Email, InvalidCredential, Password, PasswordPolicy, WeakPassword};

        fn an_ascii_token() -> impl Strategy<Value = String> {
            "[a-zA-Z0-9]{1,8}"
        }

        proptest! {
            #[test]
            fn an_email_with_a_dot_bounded_domain_round_trips_lowercased(
                local in an_ascii_token(),
                left in an_ascii_token(),
                right in an_ascii_token(),
            ) {
                let raw = format!("{local}@{left}.{right}");

                let email = Email::try_from(raw.as_str()).expect("a shaped address");

                prop_assert_eq!(email.as_str(), raw.to_ascii_lowercase());
            }

            #[test]
            fn differently_cased_and_padded_spellings_of_the_same_address_agree(
                local in an_ascii_token(),
                left in an_ascii_token(),
                right in an_ascii_token(),
            ) {
                let raw = format!("{local}@{left}.{right}");
                let padded = format!("  {}  ", raw.to_ascii_uppercase());

                let email = Email::try_from(padded.as_str()).expect("a padded, differently cased address");

                prop_assert_eq!(email.as_str(), raw.to_ascii_lowercase());
            }

            #[test]
            fn a_domain_without_a_dot_is_refused_whatever_the_rest_looks_like(
                local in an_ascii_token(),
                domain in an_ascii_token(),
            ) {
                let raw = format!("{local}@{domain}");

                prop_assert_eq!(Email::try_from(raw.as_str()), Err(InvalidCredential::Email));
            }

            #[test]
            fn a_password_with_at_least_the_floor_of_printable_characters_is_admitted(
                chars in prop::collection::vec(
                    0x21u8..=0x7eu8,
                    MIN_PASSWORD_CHARS..=MIN_PASSWORD_CHARS + 40
                )
            ) {
                let spelling = String::from_utf8(chars).expect("ascii bytes");
                let password = Password::new(&spelling);

                prop_assert_eq!(PasswordPolicy::default().admits(&password), Ok(()));
            }

            #[test]
            fn a_password_shorter_than_the_floor_is_refused_whatever_it_contains(
                chars in prop::collection::vec(0x21u8..=0x7eu8, 1..MIN_PASSWORD_CHARS)
            ) {
                let spelling = String::from_utf8(chars).expect("ascii bytes");
                let password = Password::new(&spelling);

                prop_assert_eq!(
                    PasswordPolicy::default().admits(&password),
                    Err(WeakPassword::TooShort(MIN_PASSWORD_CHARS))
                );
            }

            #[test]
            fn a_password_of_only_whitespace_is_refused_whatever_its_length(
                length in MIN_PASSWORD_CHARS..=MAX_PASSWORD_BYTES,
                space in prop_oneof![Just(' '), Just('\t'), Just('\n')],
            ) {
                let password = Password::new(&space.to_string().repeat(length));

                prop_assert_eq!(
                    PasswordPolicy::default().admits(&password),
                    Err(WeakPassword::Blank)
                );
            }
        }
    }
}
