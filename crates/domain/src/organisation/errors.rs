use crate::identity::SessionError;
use crate::profile::PhotoError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadName {
    #[error("an organisation needs a name")]
    Blank,
    #[error("an organisation name is at most {0} bytes")]
    TooLong(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("changing an organisation is an administrator's to do")]
    NotYoursToChange,
    #[error("no organisation carries that id any more")]
    Gone,
    #[error("that organisation keeps no logo")]
    NoLogo,
    #[error(transparent)]
    Photos(PhotoError),
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for SettingError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for SettingError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

impl From<PhotoError> for SettingError {
    fn from(error: PhotoError) -> Self {
        match error {
            PhotoError::NotOffered => Self::Photos(PhotoError::NotOffered),
            PhotoError::Unreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WhoamiError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("the control plane could not be reached")]
    StoreUnreachable,
}

impl From<SessionError> for WhoamiError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unknown => Self::NoSession,
            SessionError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RosterError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("changing who belongs here is for an administrator to do")]
    NotYoursToChange,
    #[error("the owner role is an owner to hand out and an owner to take back")]
    OwnersOnly,
    #[error("an organisation keeps at least one owner")]
    LastOwner,
    #[error("no member of this organisation carries that account")]
    NoSuchMember,
    #[error("that address already belongs to this organisation")]
    AlreadyAMember,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for RosterError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for RosterError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadTeamName {
    #[error("a team needs a name")]
    Blank,
    #[error("a team name is at most {0} bytes")]
    TooLong(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TeamError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("teams are an administrator's to arrange")]
    NotYoursToChange,
    #[error("no team of this organisation carries that id")]
    NoSuchTeam,
    #[error("this organisation already holds a team under that name")]
    NameTaken,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for TeamError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for TeamError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}
