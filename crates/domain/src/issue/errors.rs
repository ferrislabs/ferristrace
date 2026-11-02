use crate::explore::ReadError;
use crate::organisation::WhoamiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a fingerprint is 16 bytes, written as 32 hex characters")]
pub struct BadFingerprint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IssueError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this organisation is not yours to read")]
    NotYours,
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

impl From<ReadError> for IssueError {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::StoreUnreachable => Self::Unavailable,
        }
    }
}
