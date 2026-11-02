use core::fmt;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::errors::InvalidCredential;
use crate::clock::UnixSeconds;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Email(Box<str>);

impl Email {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for Email {
    type Error = InvalidCredential;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let trimmed = text.trim();
        let Some((local, domain)) = trimmed.split_once('@') else {
            return Err(InvalidCredential::Email);
        };
        let shaped = !local.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && !trimmed.contains(char::is_whitespace);
        if shaped {
            Ok(Self(trimmed.to_ascii_lowercase().into_boxed_str()))
        } else {
            Err(InvalidCredential::Email)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Subject(Box<str>);

impl Subject {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for Subject {
    type Error = InvalidCredential;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(InvalidCredential::Subject);
        }
        Ok(Self(trimmed.into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Issuer(Box<str>);

impl Issuer {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for Issuer {
    type Error = InvalidCredential;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(InvalidCredential::Issuer);
        }
        Ok(Self(trimmed.into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountId(Uuid);

impl AccountId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SignInProof {
    pub code: String,
    pub code_verifier: String,
    pub nonce: String,
    pub redirect_uri: String,
}

impl fmt::Debug for SignInProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SignInProof(redacted)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIdentity {
    pub issuer: Issuer,
    pub subject: Subject,
    pub email: Email,
    pub email_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProvisionPolicy {
    pub auto_provision: bool,
    pub link_by_email: bool,
}

impl Default for ProvisionPolicy {
    fn default() -> Self {
        Self {
            auto_provision: true,
            link_by_email: false,
        }
    }
}

const DEFAULT_IDLE_SECONDS: u32 = 8 * 60 * 60;

const DEFAULT_HARD_SECONDS: u32 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLifetime {
    pub idle_seconds: u32,
    pub hard_seconds: u32,
}

impl Default for SessionLifetime {
    fn default() -> Self {
        Self {
            idle_seconds: DEFAULT_IDLE_SECONDS,
            hard_seconds: DEFAULT_HARD_SECONDS,
        }
    }
}

impl SessionLifetime {
    pub fn expiry_from(self, created: UnixSeconds, now: UnixSeconds) -> UnixSeconds {
        let idle = now.saturating_add_secs(u64::from(self.idle_seconds));
        let hard = created.saturating_add_secs(u64::from(self.hard_seconds));
        if idle < hard { idle } else { hard }
    }
}

const MAX_AGENT_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent(Box<str>);

impl Agent {
    pub fn new(said: &str) -> Option<Self> {
        let trimmed = said.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut end = trimmed.len().min(MAX_AGENT_BYTES);
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        Some(Self(trimmed[..end].into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(Uuid);

impl SessionId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub id: SessionId,
    pub from: Option<Agent>,
    pub created_at: UnixSeconds,
    pub last_seen_at: UnixSeconds,
    pub current: bool,
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct CodeVerifier(String);

impl CodeVerifier {
    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CodeVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CodeVerifier(redacted)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PendingFlow {
    pub state: String,
    pub nonce: String,
    pub verifier: CodeVerifier,
    pub redirect_uri: String,
}

impl fmt::Debug for PendingFlow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingFlow")
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}
