use core::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::errors::AckRefused;
use super::value_objects::PageId;
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::telemetry::TenantId;

const VERSION: &str = "v1";
const DERIVATION: &[u8] = b"ferristrace/page-ack/v1";
const KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AckClaim {
    pub org: TenantId,
    pub page: PageId,
    pub account: AccountId,
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct AckKey([u8; KEY_BYTES]);

impl AckKey {
    pub fn derive(secret: &[u8]) -> Self {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key");
        mac.update(DERIVATION);
        Self(mac.finalize().into_bytes().into())
    }

    pub fn sign(&self, claim: &AckClaim, expires: UnixSeconds) -> Box<str> {
        let body = body(claim, expires);
        let signature = hex::encode(self.mac(&body).finalize().into_bytes());
        format!("{body}.{signature}").into()
    }

    pub fn open(&self, token: &str, now: UnixSeconds) -> Result<AckClaim, AckRefused> {
        let (body, signature) = token.rsplit_once('.').ok_or(AckRefused::Tampered)?;
        let signature = hex::decode(signature).map_err(|_| AckRefused::Tampered)?;
        self.mac(body)
            .verify_slice(&signature)
            .map_err(|_| AckRefused::Tampered)?;
        let mut parts = body.split('.');
        let (Some(VERSION), Some(org), Some(page), Some(account), Some(expires), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(AckRefused::Tampered);
        };
        let uuid = |text: &str| Uuid::try_parse(text).map_err(|_| AckRefused::Tampered);
        let claim = AckClaim {
            org: TenantId::new(uuid(org)?),
            page: PageId::new(uuid(page)?),
            account: AccountId::new(uuid(account)?),
        };
        let expires: u64 = expires.parse().map_err(|_| AckRefused::Tampered)?;
        if now.get() >= expires {
            return Err(AckRefused::Expired);
        }
        Ok(claim)
    }

    fn mac(&self, body: &str) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("hmac accepts any key");
        mac.update(body.as_bytes());
        mac
    }
}

fn body(claim: &AckClaim, expires: UnixSeconds) -> String {
    format!(
        "{VERSION}.{}.{}.{}.{}",
        claim.org.as_uuid().simple(),
        claim.page.as_uuid().simple(),
        claim.account.as_uuid().simple(),
        expires.get()
    )
}

impl fmt::Debug for AckKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AckKey(redacted)")
    }
}
