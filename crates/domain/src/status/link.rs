use core::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::errors::LinkRefused;
use super::value_objects::{StatusPageId, SubscriberId};
use crate::clock::UnixSeconds;

const VERSION: &str = "v1";
const DERIVATION: &[u8] = b"ferristrace/status-link/v1";
const KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Confirm,
    Unsubscribe,
}

impl Purpose {
    fn letter(self) -> &'static str {
        match self {
            Self::Confirm => "c",
            Self::Unsubscribe => "u",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkClaim {
    pub purpose: Purpose,
    pub page: StatusPageId,
    pub subscriber: SubscriberId,
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct LinkKey([u8; KEY_BYTES]);

impl LinkKey {
    pub fn derive(secret: &[u8]) -> Self {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key");
        mac.update(DERIVATION);
        Self(mac.finalize().into_bytes().into())
    }

    pub fn sign(&self, claim: &LinkClaim, expires: UnixSeconds) -> Box<str> {
        let body = format!(
            "{VERSION}.{}.{}.{}.{}",
            claim.purpose.letter(),
            claim.page.as_uuid().simple(),
            claim.subscriber.as_uuid().simple(),
            expires.get()
        );
        let signature = hex::encode(self.mac(&body).finalize().into_bytes());
        format!("{body}.{signature}").into()
    }

    pub fn open(
        &self,
        token: &str,
        purpose: Purpose,
        now: UnixSeconds,
    ) -> Result<LinkClaim, LinkRefused> {
        let (body, signature) = token.rsplit_once('.').ok_or(LinkRefused::Tampered)?;
        let signature = hex::decode(signature).map_err(|_| LinkRefused::Tampered)?;
        self.mac(body)
            .verify_slice(&signature)
            .map_err(|_| LinkRefused::Tampered)?;
        let mut parts = body.split('.');
        let (Some(VERSION), Some(letter), Some(page), Some(subscriber), Some(expires), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(LinkRefused::Tampered);
        };
        if letter != purpose.letter() {
            return Err(LinkRefused::Tampered);
        }
        let uuid = |text: &str| Uuid::try_parse(text).map_err(|_| LinkRefused::Tampered);
        let claim = LinkClaim {
            purpose,
            page: StatusPageId::new(uuid(page)?),
            subscriber: SubscriberId::new(uuid(subscriber)?),
        };
        let expires: u64 = expires.parse().map_err(|_| LinkRefused::Tampered)?;
        if now.get() >= expires {
            return Err(LinkRefused::Expired);
        }
        Ok(claim)
    }

    fn mac(&self, body: &str) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("hmac accepts any key");
        mac.update(body.as_bytes());
        mac
    }
}

impl fmt::Debug for LinkKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LinkKey(redacted)")
    }
}
