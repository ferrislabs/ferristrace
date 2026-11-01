use core::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::clock::UnixSeconds;
use crate::credential::Email;

pub const SECRET_BYTES: usize = 20;
pub const STEP_SECONDS: u64 = 30;

const DIGITS: u32 = 1_000_000;
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const BASE32_CHARS: usize = SECRET_BYTES * 8 / 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a code from an authenticator is six digits")]
pub struct NotACode;

#[derive(Clone, Copy, Eq)]
pub struct TotpCode(u32);

impl TotpCode {
    pub fn new(value: u32) -> Result<Self, NotACode> {
        if value < DIGITS {
            Ok(Self(value))
        } else {
            Err(NotACode)
        }
    }
}

impl TryFrom<&str> for TotpCode {
    type Error = NotACode;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let digits = text.trim();
        if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(NotACode);
        }
        digits
            .parse::<u32>()
            .map_err(|_| NotACode)
            .and_then(Self::new)
    }
}

impl PartialEq for TotpCode {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl fmt::Debug for TotpCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TotpCode(redacted)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Step(u64);

impl Step {
    pub fn at(moment: UnixSeconds) -> Self {
        Self(moment.get() / STEP_SECONDS)
    }

    pub fn new(count: u64) -> Self {
        Self(count)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    fn back(self, steps: u8) -> Self {
        Self(self.0.saturating_sub(u64::from(steps)))
    }
}

#[derive(Clone, Eq, Zeroize, ZeroizeOnDrop)]
pub struct TotpSecret([u8; SECRET_BYTES]);

impl TotpSecret {
    pub fn random() -> Self {
        Self(rand::random())
    }

    pub fn from_bytes(bytes: [u8; SECRET_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; SECRET_BYTES] {
        &self.0
    }

    pub fn base32(&self) -> String {
        let mut rendered = String::with_capacity(BASE32_CHARS);
        for chunk in self.0.chunks_exact(5) {
            let bits =
                u64::from_be_bytes([0, 0, 0, chunk[0], chunk[1], chunk[2], chunk[3], chunk[4]]);
            for place in (0..8).rev() {
                let index = u8::try_from((bits >> (place * 5)) & 0x1f).unwrap_or(0);
                rendered.push(char::from(ALPHABET[usize::from(index)]));
            }
        }
        rendered
    }

    pub fn uri(&self, issuer: &str, account: &Email) -> String {
        format!(
            "otpauth://totp/{issuer}:{}?secret={}&issuer={issuer}&algorithm=SHA1&digits=6&period={STEP_SECONDS}",
            account.as_str(),
            self.base32(),
        )
    }

    pub fn code_at(&self, step: Step) -> TotpCode {
        let mut mac =
            Hmac::<Sha1>::new_from_slice(&self.0).expect("hmac accepts a key of any length");
        mac.update(&step.get().to_be_bytes());
        let out = mac.finalize().into_bytes();
        let digest: &[u8] = &out;
        let offset = usize::from(digest[digest.len() - 1] & 0x0f);
        let truncated = u32::from_be_bytes([
            digest[offset] & 0x7f,
            digest[offset + 1],
            digest[offset + 2],
            digest[offset + 3],
        ]);
        TotpCode(truncated % DIGITS)
    }

    pub fn matched(&self, code: TotpCode, at: UnixSeconds, back: u8) -> Option<Step> {
        let now = Step::at(at);
        let earliest = now.back(back);
        (earliest.get()..=now.get())
            .rev()
            .map(Step::new)
            .find(|step| self.code_at(*step) == code)
    }
}

impl PartialEq for TotpSecret {
    fn eq(&self, other: &Self) -> bool {
        self.0[..].ct_eq(&other.0[..]).into()
    }
}

impl fmt::Debug for TotpSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TotpSecret(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use super::{NotACode, SECRET_BYTES, STEP_SECONDS, Step, TotpCode, TotpSecret};
    use crate::clock::UnixSeconds;
    use crate::credential::Email;

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
}
