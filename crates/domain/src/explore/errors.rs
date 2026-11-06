use crate::access::ReadRefusal;
use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadWindow {
    #[error("a window must end after it starts")]
    Backwards,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadQuery {
    #[error("an attribute key is 1 to 128 characters with no control character")]
    Key,
    #[error("a number must be finite")]
    Number,
    #[error("this operator does not fit this field or this value")]
    Shape,
    #[error("this value is not one the field can hold")]
    Value,
    #[error("a value is at most 512 bytes")]
    Text,
    #[error("a list holds 1 to 50 values")]
    List,
    #[error("a query nests at most 8 levels")]
    Deep,
    #[error("a query holds at most 32 conditions")]
    Large,
    #[error("a group holds at least one condition")]
    Empty,
    #[error("this field does not exist on this signal")]
    Field,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExploreError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
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

impl From<ReadRefusal> for ExploreError {
    fn from(error: ReadRefusal) -> Self {
        match error {
            ReadRefusal::Unknown => Self::NoSession,
            ReadRefusal::NotYours => Self::NotYours,
            ReadRefusal::OutOfScope => Self::OutOfScope,
            ReadRefusal::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}
