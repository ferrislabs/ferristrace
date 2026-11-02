#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidCredential {
    #[error("an email address needs a local part and a domain")]
    Email,
    #[error("a subject cannot be empty")]
    Subject,
    #[error("an issuer cannot be empty")]
    Issuer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("this way of signing in is not enabled")]
    SourceDisabled,
    #[error("the credentials match no account")]
    Rejected,
    #[error("the identity provider could not be reached")]
    ProviderUnreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("no live session carries this token")]
    Unknown,
    #[error("the session store could not be reached")]
    StoreUnreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RollError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this account keeps no such session")]
    NoSuchSession,
    #[error("the sessions could not be reached")]
    Unavailable,
}

impl From<SessionError> for RollError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unknown => Self::NoSession,
            SessionError::StoreUnreachable => Self::Unavailable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignInError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FederatedError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Flow(#[from] FlowError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FlowError {
    #[error("no attempt is waiting under this state")]
    Unknown,
    #[error("the attempt store could not be reached")]
    StoreUnreachable,
}
