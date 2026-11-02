use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadAgentName {
    #[error("an agent needs a name")]
    Blank,
    #[error("an agent name is at most {0} bytes")]
    TooLong(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadClientId {
    #[error("an agent needs the client id FerrisKey knows it by")]
    Blank,
    #[error("a client id is at most {0} bytes")]
    TooLong(usize),
    #[error("a client id holds no whitespace or control character")]
    Spaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadAgentScopes {
    #[error("an agent carries at least one scope")]
    Empty,
    #[error("an agent carries each scope once")]
    Repeated,
    #[error(
        "a scope is telemetry:read, incidents:comment, incidents:resolve or oncall:acknowledge"
    )]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("agents are an administrator's to register and revoke")]
    NotYoursToChange,
    #[error("no project of this organisation carries that id")]
    NoSuchProject,
    #[error("no live agent of this organisation carries that id")]
    NoSuchAgent,
    #[error("a live agent of this organisation already carries that client id")]
    AlreadyRegistered,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for AgentError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for AgentError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenRefusal {
    #[error("no agent issuer is configured")]
    Off,
    #[error("the token is malformed, wrongly signed, expired or not meant for this service")]
    Invalid,
    #[error("the issuer's keys could not be reached")]
    Unavailable,
}
