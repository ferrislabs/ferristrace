use crate::organisation::WhoamiError;
use crate::store::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadAccountName {
    #[error("a name cannot be only whitespace")]
    Blank,
    #[error("a name is at most {0} bytes")]
    TooLong(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("that account keeps no photo")]
    NoPhoto,
    #[error(transparent)]
    Photos(PhotoError),
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for ProfileError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<StoreError> for ProfileError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unreachable => Self::StoreUnreachable,
        }
    }
}

impl From<PhotoError> for ProfileError {
    fn from(error: PhotoError) -> Self {
        match error {
            PhotoError::NotOffered => Self::Photos(PhotoError::NotOffered),
            PhotoError::Unreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadPhoto {
    #[error("a photo is at most {0} bytes")]
    TooLarge(usize),
    #[error("a photo must be a png, a jpeg or a webp")]
    NotAnImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PhotoError {
    #[error("this deployment keeps no photos: it has no object store configured")]
    NotOffered,
    #[error("the photo store could not be reached")]
    Unreachable,
}
