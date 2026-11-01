use core::fmt;
use core::future::Future;

use uuid::Uuid;

use crate::clock::UnixSeconds;
use crate::credential::{Email, Password};
use crate::identity::AccountId;
use crate::photo::Portrait;
use crate::secret::SessionToken;
use crate::tenant::TenantId;

const DEFAULT_IDLE_SECONDS: u32 = 8 * 60 * 60;
const DEFAULT_HARD_SECONDS: u32 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("no live session carries this token")]
    Unknown,
    #[error("the session store could not be reached")]
    StoreUnreachable,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub account: AccountId,
    pub email: Email,
    pub name: Option<Box<str>>,
    pub photo: Option<Portrait>,
    pub org: Option<TenantId>,
    pub expires_at: UnixSeconds,
}

pub trait SessionStore {
    fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> impl Future<Output = Result<SessionToken, SessionError>> + Send;

    fn resolve(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<Session, SessionError>> + Send;

    fn revoke(&self, token: &SessionToken)
    -> impl Future<Output = Result<(), SessionError>> + Send;
}

impl<T: SessionStore + Sync> SessionStore for &T {
    fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> impl Future<Output = Result<SessionToken, SessionError>> + Send {
        (*self).issue(account, org, from)
    }

    fn resolve(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<Session, SessionError>> + Send {
        (*self).resolve(token)
    }

    fn revoke(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<(), SessionError>> + Send {
        (*self).revoke(token)
    }
}

impl<T: SessionStore + Send + Sync> SessionStore for std::sync::Arc<T> {
    fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> impl Future<Output = Result<SessionToken, SessionError>> + Send {
        self.as_ref().issue(account, org, from)
    }

    fn resolve(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<Session, SessionError>> + Send {
        self.as_ref().resolve(token)
    }

    fn revoke(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.as_ref().revoke(token)
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

pub trait SessionRoll {
    fn live(
        &self,
        account: AccountId,
        current: &SessionToken,
    ) -> impl Future<Output = Result<Vec<LiveSession>, SessionError>> + Send;

    fn revoke_one(
        &self,
        account: AccountId,
        id: SessionId,
    ) -> impl Future<Output = Result<bool, SessionError>> + Send;

    fn revoke_others(
        &self,
        account: AccountId,
        current: &SessionToken,
    ) -> impl Future<Output = Result<u64, SessionError>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RollError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this account keeps no such session")]
    NoSuchSession,
    #[error("the sessions could not be reached")]
    Unavailable,
}

impl From<SessionError> for RollError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unknown => Self::NoSession,
            SessionError::StoreUnreachable => Self::Unavailable,
        }
    }
}

#[derive(Debug)]
pub struct Sessions<S, R> {
    sessions: S,
    roll: R,
}

impl<S, R> Sessions<S, R>
where
    S: SessionStore,
    R: SessionRoll,
{
    pub fn new(sessions: S, roll: R) -> Self {
        Self { sessions, roll }
    }

    pub async fn live(&self, token: &SessionToken) -> Result<Vec<LiveSession>, RollError> {
        let session = self.sessions.resolve(token).await?;
        Ok(self.roll.live(session.account, token).await?)
    }

    pub async fn end_one(&self, token: &SessionToken, id: SessionId) -> Result<(), RollError> {
        let session = self.sessions.resolve(token).await?;
        if self.roll.revoke_one(session.account, id).await? {
            return Ok(());
        }
        Err(RollError::NoSuchSession)
    }

    pub async fn end_others(&self, token: &SessionToken) -> Result<u64, RollError> {
        let session = self.sessions.resolve(token).await?;
        Ok(self.roll.revoke_others(session.account, token).await?)
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HashError {
    #[error("the stored password hash is not readable")]
    Unreadable,
    #[error("the password could not be hashed")]
    Unhashable,
}

pub trait PasswordHasher {
    fn hash(
        &self,
        password: &Password,
    ) -> impl Future<Output = Result<PasswordHash, HashError>> + Send;

    fn check(
        &self,
        password: &Password,
        against: &PasswordHash,
    ) -> impl Future<Output = Result<PasswordCheck, HashError>> + Send;

    fn decoy(&self) -> &PasswordHash;
}

#[cfg(test)]
mod tests {
    use super::SessionLifetime;
    use crate::clock::UnixSeconds;

    const CREATED: u64 = 1_700_000_000;

    fn lifetime() -> SessionLifetime {
        SessionLifetime {
            idle_seconds: 100,
            hard_seconds: 1000,
        }
    }

    #[test]
    fn the_idle_window_slides_while_it_stays_inside_the_hard_life() {
        let expiry =
            lifetime().expiry_from(UnixSeconds::new(CREATED), UnixSeconds::new(CREATED + 500));

        assert_eq!(expiry, UnixSeconds::new(CREATED + 600));
    }

    #[test]
    fn the_hard_life_caps_the_sliding_window() {
        let expiry =
            lifetime().expiry_from(UnixSeconds::new(CREATED), UnixSeconds::new(CREATED + 950));

        assert_eq!(
            expiry,
            UnixSeconds::new(CREATED + 1000),
            "sliding must not reach past the hard life"
        );
    }

    #[test]
    fn a_fresh_session_expires_after_one_idle_window() {
        let created = UnixSeconds::new(CREATED);

        assert_eq!(
            lifetime().expiry_from(created, created),
            UnixSeconds::new(CREATED + 100)
        );
    }

    #[test]
    fn the_default_lifetime_is_a_working_day_inside_a_month() {
        let default = SessionLifetime::default();

        assert_eq!(default.idle_seconds, 8 * 60 * 60);
        assert_eq!(default.hard_seconds, 30 * 24 * 60 * 60);
    }
}
