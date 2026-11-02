use crate::access::ReadRefusal;
use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadStatusPage {
    #[error("a status page needs a name of at most {max} bytes")]
    Name { max: usize },
    #[error("a component needs a name of at most {max} bytes")]
    ComponentName { max: usize },
    #[error("a service link names a service of at most {max} bytes")]
    Service { max: usize },
    #[error("an introduction holds between one and {max} bytes")]
    Intro { max: usize },
    #[error(
        "a slug is 3 to 48 lowercase letters, digits or hyphens, and neither confirm nor unsubscribe"
    )]
    Slug,
    #[error("the accent is one of indigo, teal, orange, rose or slate")]
    Accent,
    #[error("the mode is one of light, dark or auto")]
    Mode,
    #[error("a component status is one of operational, degraded or down")]
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LinkRefused {
    #[error("this link was not signed by this deployment")]
    Tampered,
    #[error("this link has expired")]
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StatusError {
    #[error("neither a live session nor a live read key carries this credential")]
    NoSession,
    #[error("this reader holds no membership of that organisation")]
    NotYours,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error("a read key reads status pages; changing them takes a session")]
    ReadOnly,
    #[error("only an administrator changes a status page")]
    NotAdmin,
    #[error("no status page carries that address")]
    NoSuchPage,
    #[error("no component of this status page carries that id")]
    NoSuchComponent,
    #[error("no alert rule of this organisation carries that id")]
    NoSuchAlert,
    #[error("no project of this organisation carries that id")]
    NoSuchProject,
    #[error("no published postmortem is shown on this status page for that incident")]
    NoSuchPostmortem,
    #[error("this subscription no longer exists")]
    NoSuchSubscriber,
    #[error("another status page already uses this slug")]
    SlugTaken,
    #[error("a status page holds at most {max} components")]
    TooManyComponents { max: usize },
    #[error("an order lists every component of the page exactly once")]
    Order,
    #[error(transparent)]
    Page(#[from] BadStatusPage),
    #[error(transparent)]
    Link(#[from] LinkRefused),
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for StatusError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for StatusError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadRefusal> for StatusError {
    fn from(error: ReadRefusal) -> Self {
        match error {
            ReadRefusal::Unknown => Self::NoSession,
            ReadRefusal::NotYours => Self::NotYours,
            ReadRefusal::OutOfScope => Self::OutOfScope,
            ReadRefusal::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}
