use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadProjectName {
    #[error("a project needs a name")]
    Blank,
    #[error("a project name is at most {0} bytes")]
    TooLong(usize),
    #[error("a project name holds lowercase letters, digits and dashes, and starts with one")]
    Shape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CatalogueError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("the projects of an organisation are an administrator's to arrange")]
    NotYoursToChange,
    #[error("no project of this organisation carries that id")]
    NoSuchProject,
    #[error("no team of this organisation carries that id")]
    NoSuchTeam,
    #[error("this organisation already holds a project under that name")]
    NameTaken,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for CatalogueError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for CatalogueError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}
