use core::fmt;

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use ferristrace_domain::{AckKey, LinkKey};
use zeroize::{Zeroize, Zeroizing};

const NONCE_BYTES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a key at rest is 32 bytes, written as 64 hex characters")]
pub struct BadKey;

#[derive(Clone)]
pub struct SecretSeal {
    cipher: ChaCha20Poly1305,
}

impl SecretSeal {
    pub fn from_hex(text: &str) -> Result<Self, BadKey> {
        let mut key = hex::decode(text.trim()).map_err(|_| BadKey)?;
        let built = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| BadKey);
        key.zeroize();
        built.map(|cipher| Self { cipher })
    }

    pub fn seal_bytes(&self, clear: &[u8]) -> Option<Vec<u8>> {
        let nonce: [u8; NONCE_BYTES] = rand::random();
        let sealed = self.cipher.encrypt(&Nonce::from(nonce), clear).ok()?;
        let mut row = Vec::with_capacity(NONCE_BYTES + sealed.len());
        row.extend_from_slice(&nonce);
        row.extend_from_slice(&sealed);
        Some(row)
    }

    pub fn unseal_bytes(&self, row: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
        if row.len() <= NONCE_BYTES {
            return None;
        }
        let (head, body) = row.split_at(NONCE_BYTES);
        let nonce: [u8; NONCE_BYTES] = head.try_into().ok()?;
        self.cipher
            .decrypt(&Nonce::from(nonce), body)
            .ok()
            .map(Zeroizing::new)
    }
}

pub fn ack_key_from_hex(text: &str) -> Result<AckKey, BadKey> {
    let mut key = hex::decode(text.trim()).map_err(|_| BadKey)?;
    let derived = (key.len() == 32).then(|| AckKey::derive(&key));
    key.zeroize();
    derived.ok_or(BadKey)
}

pub fn status_key_from_hex(text: &str) -> Result<LinkKey, BadKey> {
    let mut key = hex::decode(text.trim()).map_err(|_| BadKey)?;
    let derived = (key.len() == 32).then(|| LinkKey::derive(&key));
    key.zeroize();
    derived.ok_or(BadKey)
}

impl fmt::Debug for SecretSeal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretSeal(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::{BadKey, SecretSeal};

    fn a_key() -> String {
        hex::encode([9_u8; 32])
    }

    fn another_key() -> String {
        hex::encode([4_u8; 32])
    }

    fn a_secret() -> Vec<u8> {
        vec![5_u8; 20]
    }

    #[test]
    fn the_same_secret_seals_differently_every_time() {
        let seal = SecretSeal::from_hex(&a_key()).expect("a key");
        let secret = a_secret();

        let once = seal.seal_bytes(&secret).expect("it seals");
        let twice = seal.seal_bytes(&secret).expect("it seals");

        assert_ne!(once, twice, "the nonce is fresh per write");
        assert_eq!(seal.unseal_bytes(&once), seal.unseal_bytes(&twice));
    }

    #[test]
    fn another_key_opens_nothing() {
        let mine = SecretSeal::from_hex(&a_key()).expect("a key");
        let theirs = SecretSeal::from_hex(&another_key()).expect("a key");

        let row = mine.seal_bytes(&a_secret()).expect("it seals");

        assert_eq!(theirs.unseal_bytes(&row), None);
    }

    #[test]
    fn a_row_changed_in_the_database_opens_nothing() {
        let seal = SecretSeal::from_hex(&a_key()).expect("a key");
        let mut row = seal.seal_bytes(&a_secret()).expect("it seals");
        let last = row.len() - 1;
        row[last] ^= 1;

        assert_eq!(seal.unseal_bytes(&row), None);
        assert_eq!(seal.unseal_bytes(&[]), None);
        assert_eq!(seal.unseal_bytes(&[0_u8; 12]), None);
    }

    #[test]
    fn a_key_is_sixty_four_hex_characters() {
        assert_eq!(SecretSeal::from_hex("").map(|_| ()), Err(BadKey));
        assert_eq!(SecretSeal::from_hex("not hex").map(|_| ()), Err(BadKey));
        assert_eq!(
            SecretSeal::from_hex(&hex::encode([1_u8; 16])).map(|_| ()),
            Err(BadKey)
        );
        assert!(SecretSeal::from_hex(&format!("  {}  ", a_key())).is_ok());
    }

    #[test]
    fn any_bytes_seal_and_come_back_whole() {
        let seal = SecretSeal::from_hex(&a_key()).expect("a key");
        let clear = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        let row = seal.seal_bytes(clear).expect("it seals");

        assert!(!row.windows(clear.len()).any(|window| window == clear));
        assert_eq!(
            seal.unseal_bytes(&row).as_deref().map(Vec::as_slice),
            Some(&clear[..])
        );
    }

    #[test]
    fn the_acknowledgement_key_follows_the_key_at_rest() {
        use ferristrace_domain::{AccountId, AckClaim, PageId, TenantId, UnixSeconds};
        use uuid::Uuid;

        let claim = AckClaim {
            org: TenantId::new(Uuid::from_u128(1)),
            page: PageId::new(Uuid::from_u128(2)),
            account: AccountId::new(Uuid::from_u128(3)),
        };
        let mine = super::ack_key_from_hex(&a_key()).expect("a key");
        let again = super::ack_key_from_hex(&format!(" {} ", a_key())).expect("a key");
        let theirs = super::ack_key_from_hex(&another_key()).expect("a key");

        let token = mine.sign(&claim, UnixSeconds::new(100));

        assert_eq!(again.open(&token, UnixSeconds::new(1)), Ok(claim));
        assert!(theirs.open(&token, UnixSeconds::new(1)).is_err());
        assert!(
            !token.contains(&a_key()),
            "the key at rest is not in the link"
        );
        assert_eq!(super::ack_key_from_hex("nope").map(|_| ()), Err(BadKey));
    }

    #[test]
    fn the_status_link_key_follows_the_key_at_rest() {
        use ferristrace_domain::{LinkClaim, Purpose, StatusPageId, SubscriberId, UnixSeconds};
        use uuid::Uuid;

        let claim = LinkClaim {
            purpose: Purpose::Unsubscribe,
            page: StatusPageId::new(Uuid::from_u128(1)),
            subscriber: SubscriberId::new(Uuid::from_u128(2)),
        };
        let mine = super::status_key_from_hex(&a_key()).expect("a key");
        let again = super::status_key_from_hex(&format!(" {} ", a_key())).expect("a key");
        let theirs = super::status_key_from_hex(&another_key()).expect("a key");

        let token = mine.sign(&claim, UnixSeconds::new(100));

        assert_eq!(
            again.open(&token, Purpose::Unsubscribe, UnixSeconds::new(1)),
            Ok(claim)
        );
        assert!(
            theirs
                .open(&token, Purpose::Unsubscribe, UnixSeconds::new(1))
                .is_err()
        );
        assert!(!token.contains(&a_key()));
        assert_eq!(super::status_key_from_hex("nope").map(|_| ()), Err(BadKey));
    }

    #[test]
    fn a_seal_never_renders_its_key() {
        let seal = SecretSeal::from_hex(&a_key()).expect("a key");

        assert_eq!(format!("{seal:?}"), "SecretSeal(..)");
    }
}
