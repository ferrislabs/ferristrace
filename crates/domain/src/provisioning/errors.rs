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
