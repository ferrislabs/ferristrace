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
    #[error("this token carries no account")]
    Unknown,
    #[error("the session store could not be reached")]
    StoreUnreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UserRefusal {
    #[error("signing in with an identity provider is not enabled")]
    Off,
    #[error("this token does not belong to a person")]
    NotAPerson,
    #[error("this token is not valid")]
    Invalid,
    #[error("the identity provider could not be reached")]
    Unavailable,
}
