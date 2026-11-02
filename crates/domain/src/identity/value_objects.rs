use core::fmt;
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::errors::{InvalidCredential, NotACode, WeakPassword};
use crate::clock::UnixSeconds;
use crate::secret::{SecretHash, SessionToken};

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
pub enum Issuer {
    Local,
    Federated(Box<str>),
}

impl Issuer {
    pub fn federated(url: &str) -> Result<Self, InvalidCredential> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(InvalidCredential::Issuer);
        }
        Ok(Self::Federated(trimmed.into()))
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Local => "local",
            Self::Federated(url) => url,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Password(String);

impl Password {
    pub fn new(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Password(redacted)")
    }
}

pub(super) const MIN_PASSWORD_CHARS: usize = 12;

pub(super) const MAX_PASSWORD_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordPolicy {
    pub min_chars: usize,
    pub max_bytes: usize,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_chars: MIN_PASSWORD_CHARS,
            max_bytes: MAX_PASSWORD_BYTES,
        }
    }
}

impl PasswordPolicy {
    pub fn admits(self, password: &Password) -> Result<(), WeakPassword> {
        let chosen = password.exposed();
        if chosen.len() > self.max_bytes {
            return Err(WeakPassword::TooLong(self.max_bytes));
        }
        if chosen.chars().count() < self.min_chars {
            return Err(WeakPassword::TooShort(self.min_chars));
        }
        if chosen.trim().is_empty() {
            return Err(WeakPassword::Blank);
        }
        Ok(())
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
pub enum SignInProof {
    Password {
        email: Email,
        password: Password,
    },
    Federated {
        code: String,
        code_verifier: String,
        nonce: String,
        redirect_uri: String,
    },
}

impl fmt::Debug for SignInProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password { .. } => formatter.write_str("SignInProof::Password(redacted)"),
            Self::Federated { .. } => formatter.write_str("SignInProof::Federated(redacted)"),
        }
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

pub const CHALLENGE_PREFIX: &str = "ct_2f_";

pub const RECOVERY_CODES: usize = 10;

pub(super) const DRIFT_STEPS: u8 = 1;

const TOKEN_BYTES: usize = 32;

const GROUP: usize = 5;

pub(super) const RECOVERY_ALPHABET: &[u8; 31] = b"abcdefghjkmnpqrstuvwxyz23456789";

pub(super) const RECOVERY_SPAN: u8 = 31;

const RECOVERY_CEILING: u8 = RECOVERY_SPAN * 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactorKind {
    Authenticator,
}

impl FactorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authenticator => "app",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "app" => Some(Self::Authenticator),
            _ => None,
        }
    }
}

impl fmt::Display for FactorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct ChallengeToken(String);

impl ChallengeToken {
    pub fn random() -> Self {
        let bytes: [u8; TOKEN_BYTES] = rand::random();
        Self(format!("{CHALLENGE_PREFIX}{}", hex::encode(bytes)))
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
        self.0.starts_with(CHALLENGE_PREFIX) && self.0.len() > CHALLENGE_PREFIX.len()
    }
}

impl fmt::Debug for ChallengeToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ChallengeToken(redacted)")
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct RecoveryCode(String);

impl RecoveryCode {
    pub fn random() -> Self {
        let mut rendered = String::with_capacity(GROUP * 2 + 1);
        for place in 0..GROUP * 2 {
            if place == GROUP {
                rendered.push('-');
            }
            rendered.push(symbol());
        }
        Self(rendered)
    }

    pub fn ten() -> Vec<Self> {
        (0..RECOVERY_CODES).map(|_| Self::random()).collect()
    }

    pub fn from_plaintext(plaintext: &str) -> Self {
        let kept: Vec<u8> = plaintext
            .bytes()
            .map(|byte| byte.to_ascii_lowercase())
            .filter(|byte| RECOVERY_ALPHABET.contains(byte))
            .collect();
        if kept.len() != GROUP * 2 {
            return Self(String::new());
        }
        let (head, tail) = kept.split_at(GROUP);
        let mut rendered = String::with_capacity(GROUP * 2 + 1);
        rendered.push_str(&String::from_utf8_lossy(head));
        rendered.push('-');
        rendered.push_str(&String::from_utf8_lossy(tail));
        Self(rendered)
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> SecretHash {
        SecretHash::of(&self.0)
    }

    pub fn looks_like_one(&self) -> bool {
        self.0.len() == GROUP * 2 + 1
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryCode(redacted)")
    }
}

fn symbol() -> char {
    loop {
        let drawn: u8 = rand::random();
        if drawn < RECOVERY_CEILING {
            return char::from(RECOVERY_ALPHABET[usize::from(drawn % RECOVERY_SPAN)]);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub live: Vec<FactorKind>,
    pub recovery_left: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrolment {
    pub kind: FactorKind,
    pub key: String,
    pub uri: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Confirmed {
    First { recovery: Vec<RecoveryCode> },
    Another,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Code(TotpCode),
    Recovery(RecoveryCode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending {
    pub account: AccountId,
    pub answers: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub secret: TotpSecret,
    pub confirmed: bool,
    pub last_step: Option<Step>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordCheck {
    Rejected,
    Accepted,
    AcceptedBehindCost(PasswordHash),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordHash(Box<str>);

impl PasswordHash {
    pub fn from_encoded(encoded: &str) -> Self {
        Self(encoded.into())
    }

    pub fn encoded(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    Session(SessionToken),
    SecondStep {
        challenge: ChallengeToken,
        offered: Vec<FactorKind>,
    },
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

pub const SECRET_BYTES: usize = 20;

pub const STEP_SECONDS: u64 = 30;

const DIGITS: u32 = 1_000_000;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

const BASE32_CHARS: usize = SECRET_BYTES * 8 / 5;

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
