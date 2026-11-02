use super::errors::NotACode;
use super::value_objects::{Email, SECRET_BYTES, STEP_SECONDS, Step, TotpCode, TotpSecret};
use crate::clock::UnixSeconds;

fn published() -> TotpSecret {
    let mut bytes = [0_u8; SECRET_BYTES];
    bytes.copy_from_slice(b"12345678901234567890");
    TotpSecret::from_bytes(bytes)
}

fn code(value: u32) -> TotpCode {
    TotpCode::new(value).expect("six digits")
}

#[test]
fn the_published_vectors_hold() {
    let secret = published();

    for (seconds, expected) in [
        (59_u64, 287_082_u32),
        (1_111_111_109, 81_804),
        (1_111_111_111, 50_471),
        (1_234_567_890, 5_924),
        (2_000_000_000, 279_037),
        (20_000_000_000, 353_130),
    ] {
        let step = Step::at(UnixSeconds::new(seconds));

        assert_eq!(
            secret.code_at(step),
            code(expected),
            "rfc 6238 names this code for {seconds} seconds"
        );
    }
}

#[test]
fn a_step_lasts_thirty_seconds() {
    assert_eq!(Step::at(UnixSeconds::new(0)), Step::new(0));
    assert_eq!(Step::at(UnixSeconds::new(29)), Step::new(0));
    assert_eq!(Step::at(UnixSeconds::new(30)), Step::new(1));
    assert_eq!(Step::at(UnixSeconds::new(59)), Step::new(1));
}

#[test]
fn the_code_of_the_current_step_is_matched() {
    let secret = published();
    let now = UnixSeconds::new(1_111_111_111);
    let current = secret.code_at(Step::at(now));

    assert_eq!(secret.matched(current, now, 1), Some(Step::at(now)));
}

#[test]
fn the_code_of_the_step_before_is_matched_and_the_one_before_that_is_not() {
    let secret = published();
    let now = UnixSeconds::new(1_111_111_111);
    let before = Step::new(Step::at(now).get() - 1);
    let older = Step::new(Step::at(now).get() - 2);

    assert_eq!(secret.matched(secret.code_at(before), now, 1), Some(before));
    assert_eq!(secret.matched(secret.code_at(older), now, 1), None);
}

#[test]
fn a_code_from_another_secret_is_not_matched() {
    let now = UnixSeconds::new(1_111_111_111);
    let other = TotpSecret::from_bytes([7_u8; SECRET_BYTES]);

    assert_eq!(
        published().matched(other.code_at(Step::at(now)), now, 1),
        None
    );
}

#[test]
fn the_matched_step_is_the_latest_that_answers() {
    let secret = published();
    let now = UnixSeconds::new(1_111_111_111);

    let matched = secret
        .matched(secret.code_at(Step::at(now)), now, 20)
        .expect("the current step answers");

    assert_eq!(matched, Step::at(now));
}

#[test]
fn a_secret_renders_as_base32_without_padding() {
    assert_eq!(published().base32(), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
    assert_eq!(published().base32().len(), 32);
    assert_eq!(
        TotpSecret::from_bytes([0_u8; SECRET_BYTES]).base32(),
        "A".repeat(32)
    );
}

#[test]
fn a_uri_names_the_issuer_the_account_and_the_period() {
    let account = Email::try_from("person@example.com").expect("an address");

    let uri = published().uri("Comettrace", &account);

    assert!(uri.starts_with("otpauth://totp/Comettrace:person@example.com?secret="));
    assert!(uri.contains("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"));
    assert!(uri.contains(&format!("period={STEP_SECONDS}")));
    assert!(uri.contains("algorithm=SHA1"));
    assert!(uri.contains("digits=6"));
}

#[test]
fn two_minted_secrets_differ() {
    assert_ne!(TotpSecret::random(), TotpSecret::random());
}

#[test]
fn neither_a_secret_nor_a_code_renders_itself() {
    let secret = published();

    assert_eq!(format!("{secret:?}"), "TotpSecret(redacted)");
    assert!(!format!("{secret:?}").contains("GEZD"));
    assert_eq!(format!("{:?}", code(123_456)), "TotpCode(redacted)");
}

#[test]
fn a_code_is_six_digits_and_nothing_else() {
    assert_eq!(TotpCode::try_from("012345"), Ok(code(12_345)));
    assert_eq!(TotpCode::try_from(" 123456 "), Ok(code(123_456)));
    assert_eq!(TotpCode::try_from("12345"), Err(NotACode));
    assert_eq!(TotpCode::try_from("1234567"), Err(NotACode));
    assert_eq!(TotpCode::try_from("12 456"), Err(NotACode));
    assert_eq!(TotpCode::try_from("abcdef"), Err(NotACode));
    assert_eq!(TotpCode::try_from(""), Err(NotACode));
    assert_eq!(TotpCode::new(1_000_000), Err(NotACode));
}
