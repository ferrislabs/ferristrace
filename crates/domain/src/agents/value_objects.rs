use core::fmt;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::errors::{BadAgentName, BadAgentScopes, BadClientId};
use crate::audit::Detail;
use crate::secret::KEY_PREFIX;

pub(super) const MAX_AGENT_NAME_BYTES: usize = 64;
pub(super) const MAX_CLIENT_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AgentScope {
    TelemetryRead,
    IncidentsComment,
    IncidentsResolve,
}

impl AgentScope {
    pub const ALL: [Self; 3] = [
        Self::TelemetryRead,
        Self::IncidentsComment,
        Self::IncidentsResolve,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TelemetryRead => "telemetry:read",
            Self::IncidentsComment => "incidents:comment",
            Self::IncidentsResolve => "incidents:resolve",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scope| scope.as_str() == text)
    }
}

impl fmt::Display for AgentScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentScopes(Vec<AgentScope>);

impl AgentScopes {
    pub fn new(given: &[AgentScope]) -> Result<Self, BadAgentScopes> {
        if given.is_empty() {
            return Err(BadAgentScopes::Empty);
        }
        let mut held = given.to_vec();
        held.sort_unstable();
        let before = held.len();
        held.dedup();
        if held.len() != before {
            return Err(BadAgentScopes::Repeated);
        }
        Ok(Self(held))
    }

    pub fn parse<T: AsRef<str>>(spellings: &[T]) -> Result<Self, BadAgentScopes> {
        let given = spellings
            .iter()
            .map(|spelling| AgentScope::parse(spelling.as_ref()).ok_or(BadAgentScopes::Unknown))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(&given)
    }

    pub fn contains(&self, scope: AgentScope) -> bool {
        self.0.contains(&scope)
    }

    pub fn as_slice(&self) -> &[AgentScope] {
        &self.0
    }

    pub fn within<T: AsRef<str>>(&self, claimed: &[T]) -> Vec<AgentScope> {
        self.0
            .iter()
            .copied()
            .filter(|scope| claimed.iter().any(|claim| claim.as_ref() == scope.as_str()))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentName(Box<str>);

impl AgentName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for AgentName {
    type Error = BadAgentName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim();
        if kept.len() > MAX_AGENT_NAME_BYTES {
            return Err(BadAgentName::TooLong(MAX_AGENT_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadAgentName::Blank);
        }
        Ok(Self(kept.into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientId(Box<str>);

impl ClientId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ClientId {
    type Error = BadClientId;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim();
        if kept.is_empty() {
            return Err(BadClientId::Blank);
        }
        if kept.len() > MAX_CLIENT_ID_BYTES {
            return Err(BadClientId::TooLong(MAX_CLIENT_ID_BYTES));
        }
        if kept
            .chars()
            .any(|letter| letter.is_whitespace() || letter.is_control())
        {
            return Err(BadClientId::Spaced);
        }
        Ok(Self(kept.into()))
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentId(Uuid);

impl AgentId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct AgentToken(String);

impl AgentToken {
    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }

    pub fn looks_like_one(offered: &str) -> bool {
        if offered.starts_with(KEY_PREFIX) {
            return false;
        }
        let mut parts = offered.split('.');
        let three = (parts.next(), parts.next(), parts.next(), parts.next());
        matches!(three, (Some(head), Some(body), Some(seal), None)
            if !head.is_empty() && !body.is_empty() && !seal.is_empty())
    }
}

impl fmt::Debug for AgentToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentToken(redacted)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    pub subject: Box<str>,
    pub email: Option<Box<str>>,
    pub act_sub: Option<Box<str>>,
    pub act_client_id: Option<Box<str>>,
}

impl Delegation {
    pub fn label(&self) -> &str {
        self.email.as_deref().unwrap_or(&self.subject)
    }

    #[must_use]
    pub fn stamp(&self, detail: Detail) -> Detail {
        let detail = detail.with("on_behalf_of", &self.subject);
        match &self.email {
            Some(email) => detail.with("on_behalf_of_email", email),
            None => detail,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    pub client_id: Box<str>,
    pub scopes: Vec<Box<str>>,
    pub jti: Box<str>,
    pub expires_at_unix_secs: u64,
    pub delegated: Option<Delegation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCall {
    pub token: VerifiedToken,
    pub route: Box<str>,
}

impl AgentCall {
    pub fn delegated(&self) -> Option<&Delegation> {
        self.token.delegated.as_ref()
    }
}
