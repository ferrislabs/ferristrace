use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadViewName {
    #[error("a view needs a name")]
    Empty,
    #[error("a view name is at most 80 bytes")]
    TooLong,
    #[error("a view name holds no control character")]
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadViewDefinition {
    #[error("a view definition is at most 16 KiB")]
    TooLarge,
    #[error("a view definition is a JSON object")]
    NotAnObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ViewError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("no such view")]
    NoSuchView,
    #[error("only its creator, an owner or an administrator may change this view")]
    NotYoursToChange,
    #[error("only the creator of a view may change who sees it")]
    NotYoursToShare,
    #[error("a view of that name already exists")]
    NameTaken,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for ViewError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for ViewError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}
