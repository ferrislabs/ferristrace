use core::fmt;

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use ferristrace_domain::{SECRET_BYTES, TotpSecret};
use zeroize::Zeroize;

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

    pub fn seal(&self, secret: &TotpSecret) -> Option<Vec<u8>> {
        let nonce: [u8; NONCE_BYTES] = rand::random();
        let sealed = self
            .cipher
            .encrypt(&Nonce::from(nonce), secret.as_bytes().as_slice())
            .ok()?;
        let mut row = Vec::with_capacity(NONCE_BYTES + sealed.len());
        row.extend_from_slice(&nonce);
        row.extend_from_slice(&sealed);
        Some(row)
    }

    pub fn unseal(&self, row: &[u8]) -> Option<TotpSecret> {
        if row.len() <= NONCE_BYTES {
            return None;
        }
        let (head, body) = row.split_at(NONCE_BYTES);
        let nonce: [u8; NONCE_BYTES] = head.try_into().ok()?;
        let mut opened = self.cipher.decrypt(&Nonce::from(nonce), body).ok()?;
        let bytes: Option<[u8; SECRET_BYTES]> = opened.as_slice().try_into().ok();
        opened.zeroize();
        Some(TotpSecret::from_bytes(bytes?))
    }
}

impl fmt::Debug for SecretSeal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretSeal(..)")
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{SECRET_BYTES, TotpSecret};

    use super::{BadKey, SecretSeal};

    fn a_key() -> String {
        hex::encode([9_u8; 32])
    }

    fn another_key() -> String {
        hex::encode([4_u8; 32])
    }

    fn a_secret() -> TotpSecret {
        TotpSecret::from_bytes([5_u8; SECRET_BYTES])
    }

    #[test]
    fn a_sealed_secret_comes_back_whole() {
        let seal = SecretSeal::from_hex(&a_key()).expect("a key");
        let secret = a_secret();

        let row = seal.seal(&secret).expect("it seals");

        assert_eq!(seal.unseal(&row), Some(secret));
    }

    #[test]
    fn a_row_carries_neither_the_bytes_nor_their_base32() {
        let seal = SecretSeal::from_hex(&a_key()).expect("a key");
        let secret = a_secret();

        let row = seal.seal(&secret).expect("it seals");

        assert!(
            !row.windows(SECRET_BYTES)
                .any(|window| window == secret.as_bytes().as_slice()),
            "the clear secret is not in the row"
        );
        assert!(
            !String::from_utf8_lossy(&row).contains(&secret.base32()),
            "nor is its base32"
        );
        assert!(row.len() > SECRET_BYTES);
    }

    #[test]
    fn the_same_secret_seals_differently_every_time() {
        let seal = SecretSeal::from_hex(&a_key()).expect("a key");
        let secret = a_secret();

        let once = seal.seal(&secret).expect("it seals");
        let twice = seal.seal(&secret).expect("it seals");

        assert_ne!(once, twice, "the nonce is fresh per write");
        assert_eq!(seal.unseal(&once), seal.unseal(&twice));
    }

    #[test]
    fn another_key_opens_nothing() {
        let mine = SecretSeal::from_hex(&a_key()).expect("a key");
        let theirs = SecretSeal::from_hex(&another_key()).expect("a key");

        let row = mine.seal(&a_secret()).expect("it seals");

        assert_eq!(theirs.unseal(&row), None);
    }

    #[test]
    fn a_row_changed_in_the_database_opens_nothing() {
        let seal = SecretSeal::from_hex(&a_key()).expect("a key");
        let mut row = seal.seal(&a_secret()).expect("it seals");
        let last = row.len() - 1;
        row[last] ^= 1;

        assert_eq!(seal.unseal(&row), None);
        assert_eq!(seal.unseal(&[]), None);
        assert_eq!(seal.unseal(&[0_u8; 12]), None);
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
    fn a_seal_never_renders_its_key() {
        let seal = SecretSeal::from_hex(&a_key()).expect("a key");

        assert_eq!(format!("{seal:?}"), "SecretSeal(..)");
    }
}
