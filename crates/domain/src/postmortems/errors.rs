use crate::access::ReadRefusal;
use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadPostmortem {
    #[error("a postmortem holds between one and {max} bytes of markdown")]
    Body { max: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PostmortemError {
    #[error("neither a live session nor a live read key carries this credential")]
    NoSession,
    #[error("this reader holds no membership of that organisation")]
    NotYours,
    #[error("a read key reads postmortems; changing them takes a session")]
    ReadOnly,
    #[error("only an administrator does this")]
    NotAdmin,
    #[error("no incident of this organisation carries that id")]
    NoSuchIncident,
    #[error("this incident has no postmortem yet")]
    NoSuchPostmortem,
    #[error("this postmortem was edited; an administrator regenerates it with force")]
    Edited,
    #[error("a postmortem was drafted for this incident in the meantime")]
    Exists,
    #[error("this postmortem is published and read-only; unpublish it first")]
    Published,
    #[error(transparent)]
    Postmortem(#[from] BadPostmortem),
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for PostmortemError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for PostmortemError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadRefusal> for PostmortemError {
    fn from(error: ReadRefusal) -> Self {
        match error {
            ReadRefusal::Unknown => Self::NoSession,
            ReadRefusal::NotYours => Self::NotYours,
            ReadRefusal::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}
