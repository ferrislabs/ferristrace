use super::errors::InvalidCredential;
use super::value_objects::{Email, Issuer, Subject};

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
fn an_issuer_is_the_url_it_was_given_trimmed() {
    let issuer = Issuer::new("  https://id.example ").expect("a url");

    assert_eq!(issuer.as_str(), "https://id.example");
    assert_eq!(Issuer::new("  "), Err(InvalidCredential::Issuer));
}

mod proptests {
    use proptest::prelude::*;

    use super::{Email, InvalidCredential};

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
    }
}
