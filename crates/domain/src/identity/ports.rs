use core::future::Future;

use super::entities::Session;
use super::errors::{AuthError, SessionError, UserRefusal};
use super::value_objects::{AccountId, ProvisionPolicy, ResolvedIdentity};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

pub trait UserTokens {
    fn verify(&self, token: &str)
    -> impl Future<Output = Result<SessionToken, UserRefusal>> + Send;
}

impl<T: UserTokens + Sync> UserTokens for &T {
    fn verify(
        &self,
        token: &str,
    ) -> impl Future<Output = Result<SessionToken, UserRefusal>> + Send {
        (*self).verify(token)
    }
}

impl<T: UserTokens + Send + Sync> UserTokens for std::sync::Arc<T> {
    fn verify(
        &self,
        token: &str,
    ) -> impl Future<Output = Result<SessionToken, UserRefusal>> + Send {
        self.as_ref().verify(token)
    }
}

pub trait SessionStore {
    fn resolve(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<Session, SessionError>> + Send;
}

impl<T: SessionStore + Sync> SessionStore for &T {
    fn resolve(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<Session, SessionError>> + Send {
        (*self).resolve(token)
    }
}

impl<T: SessionStore + Send + Sync> SessionStore for std::sync::Arc<T> {
    fn resolve(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<Session, SessionError>> + Send {
        self.as_ref().resolve(token)
    }
}

pub trait SessionSwitch {
    fn switch(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;
}

pub trait AccountDirectory {
    fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> impl Future<Output = Result<AccountId, AuthError>> + Send;
}

impl<T: AccountDirectory + Sync> AccountDirectory for &T {
    fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> impl Future<Output = Result<AccountId, AuthError>> + Send {
        (*self).provision(identity, policy)
    }
}

impl<T: AccountDirectory + Send + Sync> AccountDirectory for std::sync::Arc<T> {
    fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> impl Future<Output = Result<AccountId, AuthError>> + Send {
        self.as_ref().provision(identity, policy)
    }
}
