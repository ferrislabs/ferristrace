use core::fmt;

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const SESSION_PREFIX: &str = "ct_ss_";

#[derive(Clone, Copy, Eq)]
pub struct SecretHash([u8; 32]);

impl SecretHash {
    pub fn of(plaintext: &str) -> Self {
        Self(Sha256::digest(plaintext.as_bytes()).into())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl PartialEq for SecretHash {
    fn eq(&self, other: &Self) -> bool {
        self.0[..].ct_eq(&other.0[..]).into()
    }
}

impl fmt::Debug for SecretHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretHash(..)")
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct SessionToken(String);

impl SessionToken {
    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> SecretHash {
        SecretHash::of(&self.0)
    }

    pub fn looks_like_one(&self) -> bool {
        self.0.starts_with(SESSION_PREFIX) && self.0.len() > SESSION_PREFIX.len()
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionToken(redacted)")
    }
}

pub const RESET_PREFIX: &str = "ct_pr_";

const TOKEN_BYTES: usize = 32;

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct ResetToken(String);

impl ResetToken {
    pub fn random() -> Self {
        let bytes: [u8; TOKEN_BYTES] = rand::random();
        Self(format!("{RESET_PREFIX}{}", hex::encode(bytes)))
    }

    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> SecretHash {
        SecretHash::of(&self.0)
    }

    pub fn looks_like_one(&self) -> bool {
        self.0.starts_with(RESET_PREFIX) && self.0.len() > RESET_PREFIX.len()
    }
}

impl fmt::Debug for ResetToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResetToken(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use super::{SESSION_PREFIX, SecretHash, SessionToken};

    #[test]
    fn the_same_secret_hashes_the_same_way() {
        assert_eq!(SecretHash::of("secret"), SecretHash::of("secret"));
        assert_ne!(SecretHash::of("secret"), SecretHash::of("Secret"));
    }

    #[test]
    fn neither_a_hash_nor_a_token_renders_itself() {
        let token = SessionToken::from_plaintext("ct_ss_abcdef");

        assert_eq!(format!("{token:?}"), "SessionToken(redacted)");
        assert_eq!(format!("{:?}", token.hash()), "SecretHash(..)");
    }

    #[test]
    fn a_token_carries_its_prefix() {
        assert!(SessionToken::from_plaintext("ct_ss_abcdef").looks_like_one());
        assert!(!SessionToken::from_plaintext("abcdef").looks_like_one());
        assert!(!SessionToken::from_plaintext(SESSION_PREFIX).looks_like_one());
    }

    #[test]
    fn a_token_hashes_to_the_digest_of_its_text() {
        let token = SessionToken::from_plaintext("ct_ss_abcdef");

        assert_eq!(token.hash(), SecretHash::of("ct_ss_abcdef"));
    }
}

#[cfg(test)]
mod reset_tests {
    use super::{RESET_PREFIX, ResetToken};

    #[test]
    fn a_minted_reset_token_carries_its_prefix_and_enough_randomness() {
        let token = ResetToken::random();

        assert!(token.looks_like_one());
        assert_eq!(token.exposed().len(), RESET_PREFIX.len() + 64);
    }

    #[test]
    fn two_minted_reset_tokens_differ() {
        assert_ne!(
            ResetToken::random().exposed(),
            ResetToken::random().exposed()
        );
    }

    #[test]
    fn a_reset_token_never_renders_itself() {
        assert_eq!(
            format!("{:?}", ResetToken::random()),
            "ResetToken(redacted)"
        );
    }
}
