use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadKeyName {
    #[error("a key needs a name")]
    Blank,
    #[error("a key name is at most {0} bytes")]
    TooLong(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadScopes {
    #[error("a key carries at least one scope")]
    Empty,
    #[error("a key carries each scope once")]
    Repeated,
    #[error("a scope is ingest or read")]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("keys are an administrator's to issue and revoke")]
    NotYoursToChange,
    #[error("no project of this organisation carries that id")]
    NoSuchProject,
    #[error("no live key of this organisation carries that id")]
    NoSuchKey,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for KeyError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for KeyError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReadRefusal {
    #[error("neither a live session, a live read key nor a live agent carries this credential")]
    Unknown,
    #[error("this reader holds no grant over that organisation or project")]
    NotYours,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for ReadRefusal {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::Unknown,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for ReadRefusal {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}
