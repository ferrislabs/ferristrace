use crate::access::ReadRefusal;
use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadIncident {
    #[error("an incident needs a title of at most {max} bytes")]
    Title { max: usize },
    #[error("a comment holds between one and {max} bytes")]
    Comment { max: usize },
    #[error("an attachment needs a label of at most {max} bytes")]
    Label { max: usize },
    #[error("a trace is attached by its 32 hexadecimal digit id")]
    TraceId,
    #[error("a logs or metric attachment carries a query of at most {max} bytes")]
    Reference { max: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IncidentError {
    #[error("neither a live session nor a live read key carries this credential")]
    NoSession,
    #[error("this reader holds no membership of that organisation")]
    NotYours,
    #[error("a read key reads incidents; changing them takes a session")]
    ReadOnly,
    #[error("no incident of this organisation carries that id")]
    NoSuchIncident,
    #[error("no project of this organisation carries that id")]
    NoSuchProject,
    #[error("no alert rule of this organisation carries that id")]
    NoSuchAlert,
    #[error("an incident is declared from an alert while it fires")]
    AlertNotFiring,
    #[error("an incident is owned by a member of its organisation")]
    NotAMember,
    #[error(transparent)]
    Incident(#[from] BadIncident),
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for IncidentError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for IncidentError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadRefusal> for IncidentError {
    fn from(error: ReadRefusal) -> Self {
        match error {
            ReadRefusal::Unknown => Self::NoSession,
            ReadRefusal::NotYours => Self::NotYours,
            ReadRefusal::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}
