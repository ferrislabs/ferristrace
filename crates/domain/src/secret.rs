use core::fmt;

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::clock::UnixSeconds;
use crate::identity::ResolvedIdentity;
#[cfg(any(test, feature = "testing"))]
use crate::identity::{Email, Issuer, Subject};

#[cfg(any(test, feature = "testing"))]
const TESTING_ISSUER: &str = "https://issuer.ferristrace.test";

#[cfg(any(test, feature = "testing"))]
const TESTING_EXPIRY: u64 = 4_102_444_800;

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

#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken {
    identity: ResolvedIdentity,
    expires_at: UnixSeconds,
}

impl SessionToken {
    pub fn verified(identity: ResolvedIdentity, expires_at: UnixSeconds) -> Self {
        Self {
            identity,
            expires_at,
        }
    }

    pub fn identity(&self) -> &ResolvedIdentity {
        &self.identity
    }

    pub fn expires_at(&self) -> UnixSeconds {
        self.expires_at
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn for_subject(subject: &str) -> Self {
        Self::verified(
            ResolvedIdentity {
                issuer: Issuer::new(TESTING_ISSUER).expect("the testing issuer is not empty"),
                subject: Subject::try_from(subject).expect("a testing subject is not empty"),
                email: Email::try_from(format!("{subject}@ferristrace.test").as_str())
                    .expect("a testing subject makes an address"),
                email_verified: true,
            },
            UnixSeconds::new(TESTING_EXPIRY),
        )
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionToken(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use super::{SecretHash, SessionToken};

    #[test]
    fn the_same_secret_hashes_the_same_way() {
        assert_eq!(SecretHash::of("secret"), SecretHash::of("secret"));
        assert_ne!(SecretHash::of("secret"), SecretHash::of("Secret"));
    }

    #[test]
    fn neither_a_hash_nor_a_token_renders_itself() {
        let token = SessionToken::for_subject("alice");

        assert_eq!(format!("{token:?}"), "SessionToken(redacted)");
        assert_eq!(format!("{:?}", SecretHash::of("secret")), "SecretHash(..)");
    }

    #[test]
    fn a_token_names_the_person_it_was_verified_for() {
        let token = SessionToken::for_subject("alice");

        assert_eq!(token.identity().subject.as_str(), "alice");
        assert_eq!(token.identity().email.as_str(), "alice@ferristrace.test");
        assert_eq!(token, SessionToken::for_subject("alice"));
        assert_ne!(token, SessionToken::for_subject("bob"));
    }
}
