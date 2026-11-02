use core::fmt;

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const SESSION_PREFIX: &str = "ft_ss_";

pub const KEY_PREFIX: &str = "ft_live_";

pub(crate) fn split_key(plaintext: &str) -> Option<(&str, &str)> {
    let (prefix, secret) = plaintext.split_once('.')?;
    if prefix.is_empty() || secret.is_empty() {
        return None;
    }
    Some((prefix, secret))
}

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
        let token = SessionToken::from_plaintext("ft_ss_abcdef");

        assert_eq!(format!("{token:?}"), "SessionToken(redacted)");
        assert_eq!(format!("{:?}", token.hash()), "SecretHash(..)");
    }

    #[test]
    fn a_token_carries_its_prefix() {
        assert!(SessionToken::from_plaintext("ft_ss_abcdef").looks_like_one());
        assert!(!SessionToken::from_plaintext("abcdef").looks_like_one());
        assert!(!SessionToken::from_plaintext(SESSION_PREFIX).looks_like_one());
    }

    #[test]
    fn a_token_hashes_to_the_digest_of_its_text() {
        let token = SessionToken::from_plaintext("ft_ss_abcdef");

        assert_eq!(token.hash(), SecretHash::of("ft_ss_abcdef"));
    }
}
