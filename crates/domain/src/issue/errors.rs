use crate::access::ReadRefusal;
use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a fingerprint is 16 bytes, written as 32 hex characters")]
pub struct BadFingerprint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IssueError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this organisation is not yours to read")]
    NotYours,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error("no issue carries that fingerprint")]
    NoSuchIssue,
    #[error("the issues could not be read")]
    Unavailable,
}

impl From<WhoamiError> for IssueError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl From<StoreError> for IssueError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::Unavailable,
        }
    }
}

impl From<ReadRefusal> for IssueError {
    fn from(error: ReadRefusal) -> Self {
        match error {
            ReadRefusal::Unknown => Self::NoSession,
            ReadRefusal::NotYours => Self::NotYours,
            ReadRefusal::OutOfScope => Self::OutOfScope,
            ReadRefusal::StoreUnreachable => Self::Unavailable,
        }
    }
}
