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
pub enum WeakPassword {
    #[error("a password needs at least {0} characters")]
    TooShort(usize),
    #[error("a password cannot be longer than {0} bytes")]
    TooLong(usize),
    #[error("a password cannot be only spaces")]
    Blank,
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
pub enum ChangeError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("that is not the current password")]
    NotTheCurrent,
    #[error(transparent)]
    Weak(#[from] WeakPassword),
    #[error("the password could not be changed")]
    Unavailable,
}

impl From<SessionError> for ChangeError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unknown => Self::NoSession,
            SessionError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl From<ResetError> for ChangeError {
    fn from(error: ResetError) -> Self {
        match error {
            ResetError::Weak(weak) => Self::Weak(weak),
            ResetError::Unknown
            | ResetError::StoreUnreachable
            | ResetError::Undeliverable
            | ResetError::Hashing(_) => Self::Unavailable,
        }
    }
}

impl From<HashError> for ChangeError {
    fn from(_: HashError) -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ResetError {
    #[error("no live reset carries this token")]
    Unknown,
    #[error("the control plane could not be reached")]
    StoreUnreachable,
    #[error("the message could not be sent")]
    Undeliverable,
    #[error(transparent)]
    Hashing(#[from] HashError),
    #[error(transparent)]
    Weak(#[from] WeakPassword),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FactorError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("no live challenge carries this token")]
    NoChallenge,
    #[error("that answer opens nothing")]
    Refused,
    #[error("this challenge has taken all the answers it will take")]
    Exhausted,
    #[error("this account keeps no such factor")]
    NoFactor,
    #[error("this account already keeps that factor")]
    AlreadyKept,
    #[error("this deployment cannot offer a second factor")]
    NotOffered,
    #[error("the control plane could not be reached")]
    StoreUnreachable,
}

impl From<SessionError> for FactorError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unknown => Self::NoSession,
            SessionError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
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
pub enum HashError {
    #[error("the stored password hash is not readable")]
    Unreadable,
    #[error("the password could not be hashed")]
    Unhashable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignInError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Factor(#[from] FactorError),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a code from an authenticator is six digits")]
pub struct NotACode;
