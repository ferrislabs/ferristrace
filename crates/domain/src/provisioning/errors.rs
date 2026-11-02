use crate::identity::SessionError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProvisionError {
    #[error("no account carries that address")]
    NoSuchAccount,
    #[error("no organisation carries that id")]
    NoSuchOrg,
    #[error("that name is already taken")]
    Taken,
    #[error("the control plane did not answer")]
    Unreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FoundError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("the control plane did not answer")]
    Unreachable,
}

impl From<SessionError> for FoundError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unknown => Self::NoSession,
            SessionError::StoreUnreachable => Self::Unreachable,
        }
    }
}

impl From<ProvisionError> for FoundError {
    fn from(error: ProvisionError) -> Self {
        match error {
            ProvisionError::NoSuchAccount => Self::NoSession,
            ProvisionError::NoSuchOrg | ProvisionError::Taken | ProvisionError::Unreachable => {
                Self::Unreachable
            }
        }
    }
}
