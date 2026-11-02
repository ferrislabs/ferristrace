use crate::access::ReadRefusal;
use crate::explore::ExploreError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReleaseError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("this organisation never saw that release")]
    Unknown,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<ReadRefusal> for ReleaseError {
    fn from(error: ReadRefusal) -> Self {
        match ExploreError::from(error) {
            ExploreError::NoSession => Self::NoSession,
            ExploreError::NotYours => Self::NotYours,
            ExploreError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for ReleaseError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}
