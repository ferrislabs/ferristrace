#[cfg(test)]
mod credential_tests;
mod entities;
mod errors;
mod federated_sign_in;
#[cfg(test)]
mod oidc_tests;
mod ports;
#[cfg(test)]
mod session_tests;
mod sessions;
mod value_objects;

pub use entities::Session;
pub use errors::{
    AuthError, FederatedError, FlowError, InvalidCredential, RollError, SessionError,
};
pub use federated_sign_in::FederatedSignIn;
pub use ports::{
    AccountDirectory, AuthProvider, Authorizer, OidcFlows, SessionRoll, SessionStore, SessionSwitch,
};
pub use sessions::Sessions;
pub use value_objects::{
    AccountId, Agent, CodeVerifier, Email, Issuer, LiveSession, PendingFlow, ProvisionPolicy,
    ResolvedIdentity, SessionId, SessionLifetime, SignInProof, Subject,
};
