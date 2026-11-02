use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadWindow {
    #[error("a window must end after it starts")]
    Backwards,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExploreError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for ExploreError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for ExploreError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}
