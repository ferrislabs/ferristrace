use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InboxError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this reader holds no membership of that organisation")]
    NotYours,
    #[error("no notification of this account carries that id")]
    NoSuchItem,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for InboxError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for InboxError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}
