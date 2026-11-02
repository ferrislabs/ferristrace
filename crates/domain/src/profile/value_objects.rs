use core::fmt;

use super::errors::{BadAccountName, BadPhoto};
use crate::identity::AccountId;
use crate::telemetry::TenantId;

pub(super) const MAX_NAME_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountName(Box<str>);

impl AccountName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for AccountName {
    type Error = BadAccountName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim();
        if kept.len() > MAX_NAME_BYTES {
            return Err(BadAccountName::TooLong(MAX_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadAccountName::Blank);
        }
        Ok(Self(kept.into()))
    }
}

pub const MAX_PHOTO_BYTES: usize = 1024 * 1024;

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

const JPEG: &[u8] = &[0xff, 0xd8, 0xff];

const RIFF: &[u8] = b"RIFF";

const WEBP: &[u8] = b"WEBP";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoKind {
    Png,
    Jpeg,
    Webp,
}

impl PhotoKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }

    pub fn named(spelling: &str) -> Option<Self> {
        match spelling {
            "image/png" => Some(Self::Png),
            "image/jpeg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            _ => None,
        }
    }

    fn sniffed(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(PNG) {
            return Some(Self::Png);
        }
        if bytes.starts_with(JPEG) {
            return Some(Self::Jpeg);
        }
        if bytes.starts_with(RIFF) && bytes.len() > 12 && &bytes[8..12] == WEBP {
            return Some(Self::Webp);
        }
        None
    }
}

impl fmt::Display for PhotoKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Photo {
    kind: PhotoKind,
    bytes: Vec<u8>,
}

impl fmt::Debug for Photo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Photo")
            .field("kind", &self.kind)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl Photo {
    pub fn examined(bytes: Vec<u8>) -> Result<Self, BadPhoto> {
        if bytes.len() > MAX_PHOTO_BYTES {
            return Err(BadPhoto::TooLarge(MAX_PHOTO_BYTES));
        }
        let kind = PhotoKind::sniffed(&bytes).ok_or(BadPhoto::NotAnImage)?;
        Ok(Self { kind, bytes })
    }

    pub fn kind(&self) -> PhotoKind {
        self.kind
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portrait {
    pub token: Box<str>,
    pub kind: PhotoKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pictured {
    Account(AccountId),
    Org(TenantId),
}
