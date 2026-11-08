use core::fmt;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::errors::{BadKeyName, BadScopes, ReadRefusal};
use crate::agents::AgentCall;
use crate::catalogue::ProjectId;
use crate::secret::{KEY_PREFIX, SessionToken, split_key};
use crate::telemetry::TenantId;

pub(super) const MAX_KEY_NAME_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyScope {
    Ingest,
    Read,
}

impl KeyScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ingest => "ingest",
            Self::Read => "read",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "ingest" => Some(Self::Ingest),
            "read" => Some(Self::Read),
            _ => None,
        }
    }
}

impl fmt::Display for KeyScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyScopes(Vec<KeyScope>);

impl KeyScopes {
    pub fn new(given: &[KeyScope]) -> Result<Self, BadScopes> {
        if given.is_empty() {
            return Err(BadScopes::Empty);
        }
        let mut held = given.to_vec();
        held.sort_unstable();
        let before = held.len();
        held.dedup();
        if held.len() != before {
            return Err(BadScopes::Repeated);
        }
        Ok(Self(held))
    }

    pub fn parse<T: AsRef<str>>(spellings: &[T]) -> Result<Self, BadScopes> {
        let given = spellings
            .iter()
            .map(|spelling| KeyScope::parse(spelling.as_ref()).ok_or(BadScopes::Unknown))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(&given)
    }

    pub fn ingest() -> Self {
        Self(vec![KeyScope::Ingest])
    }

    pub fn contains(&self, scope: KeyScope) -> bool {
        self.0.contains(&scope)
    }

    pub fn as_slice(&self) -> &[KeyScope] {
        &self.0
    }
}

const NANOS_PER_DAY: u64 = 86_400 * 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyTtl {
    Days30,
    Days90,
    Days365,
    Never,
}

impl KeyTtl {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Days30 => "30d",
            Self::Days90 => "90d",
            Self::Days365 => "365d",
            Self::Never => "never",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "30d" => Some(Self::Days30),
            "90d" => Some(Self::Days90),
            "365d" => Some(Self::Days365),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    /// `None` for `Never` - a key with no deadline to check against.
    pub fn expires_at(self, issued_at_unix_nanos: u64) -> Option<u64> {
        let days = match self {
            Self::Never => return None,
            Self::Days30 => 30,
            Self::Days90 => 90,
            Self::Days365 => 365,
        };
        Some(issued_at_unix_nanos.saturating_add(days * NANOS_PER_DAY))
    }
}

impl fmt::Display for KeyTtl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyName(Box<str>);

impl KeyName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for KeyName {
    type Error = BadKeyName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim();
        if kept.len() > MAX_KEY_NAME_BYTES {
            return Err(BadKeyName::TooLong(MAX_KEY_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadKeyName::Blank);
        }
        Ok(Self(kept.into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(Uuid);

impl KeyId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct ApiSecret(String);

impl ApiSecret {
    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }

    pub fn split(&self) -> Option<(&str, &str)> {
        split_key(&self.0)
    }

    pub fn looks_like_one(&self) -> bool {
        self.split().is_some_and(|(prefix, _)| {
            prefix.starts_with(KEY_PREFIX) && prefix.len() > KEY_PREFIX.len()
        })
    }
}

impl fmt::Debug for ApiSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiSecret(redacted)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadGrant {
    pub org: TenantId,
    pub project: Option<ProjectId>,
}

impl ReadGrant {
    pub fn whole(org: TenantId) -> Self {
        Self { org, project: None }
    }

    pub fn narrow(&self, asked: Option<ProjectId>) -> Result<Option<ProjectId>, ReadRefusal> {
        match (self.project, asked) {
            (None, asked) => Ok(asked),
            (Some(bound), None) => Ok(Some(bound)),
            (Some(bound), Some(asked)) if bound == asked => Ok(Some(bound)),
            (Some(_), Some(_)) => Err(ReadRefusal::NotYours),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reader {
    Session(SessionToken),
    Key(ApiSecret),
    Agent(AgentCall),
}
