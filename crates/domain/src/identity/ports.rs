use core::future::Future;

use super::entities::Session;
use super::errors::{AuthError, FactorError, FlowError, HashError, ResetError, SessionError};
use super::value_objects::{
    AccountId, Agent, Email, FactorKind, Kept, LiveSession, Password, PasswordCheck, PasswordHash,
    Pending, PendingFlow, ProvisionPolicy, ResolvedIdentity, SessionId, SignInProof, Step,
    TotpSecret,
};
use crate::secret::{ResetToken, SecretHash, SessionToken};
use crate::telemetry::TenantId;

pub trait AuthProvider {
    fn sign_in(
        &self,
        proof: SignInProof,
    ) -> impl Future<Output = Result<ResolvedIdentity, AuthError>> + Send;
}

impl<T: AuthProvider + Sync> AuthProvider for &T {
    fn sign_in(
        &self,
        proof: SignInProof,
    ) -> impl Future<Output = Result<ResolvedIdentity, AuthError>> + Send {
        (*self).sign_in(proof)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoProvider;

impl AuthProvider for NoProvider {
    async fn sign_in(&self, _proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        Err(AuthError::SourceDisabled)
    }
}

pub trait ResetStore {
    fn account_for(
        &self,
        email: &Email,
    ) -> impl Future<Output = Result<Option<AccountId>, ResetError>> + Send;

    fn begin(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<(), ResetError>> + Send;

    fn take(
        &self,
        digest: SecretHash,
    ) -> impl Future<Output = Result<AccountId, ResetError>> + Send;

    fn set_password(
        &self,
        account: AccountId,
        hash: &PasswordHash,
    ) -> impl Future<Output = Result<(), ResetError>> + Send;

    fn revoke_sessions(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<u64, ResetError>> + Send;
}

pub trait Mailer {
    fn send_password_reset(
        &self,
        to: &Email,
        token: &ResetToken,
    ) -> impl Future<Output = Result<(), ResetError>> + Send;
}

pub trait Gate {
    fn offered(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<FactorKind>, FactorError>> + Send;

    fn open(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<(), FactorError>> + Send;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoSecondFactor;

impl Gate for NoSecondFactor {
    async fn offered(&self, _account: AccountId) -> Result<Vec<FactorKind>, FactorError> {
        Ok(Vec::new())
    }

    async fn open(&self, _account: AccountId, _digest: SecretHash) -> Result<(), FactorError> {
        Err(FactorError::NotOffered)
    }
}

pub trait FactorStore {
    fn live(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<FactorKind>, FactorError>> + Send;

    fn keep(
        &self,
        account: AccountId,
        kind: FactorKind,
        secret: &TotpSecret,
    ) -> impl Future<Output = Result<(), FactorError>> + Send;

    fn kept(
        &self,
        account: AccountId,
        kind: FactorKind,
    ) -> impl Future<Output = Result<Option<Kept>, FactorError>> + Send;

    fn confirm(
        &self,
        account: AccountId,
        kind: FactorKind,
    ) -> impl Future<Output = Result<(), FactorError>> + Send;

    fn forget(
        &self,
        account: AccountId,
        kind: FactorKind,
    ) -> impl Future<Output = Result<bool, FactorError>> + Send;

    fn spent_step(
        &self,
        account: AccountId,
        kind: FactorKind,
        step: Step,
    ) -> impl Future<Output = Result<(), FactorError>> + Send;

    fn keep_recovery(
        &self,
        account: AccountId,
        digests: &[SecretHash],
    ) -> impl Future<Output = Result<(), FactorError>> + Send;

    fn spend_recovery(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<bool, FactorError>> + Send;

    fn recovery_left(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<u32, FactorError>> + Send;

    fn answered(
        &self,
        digest: SecretHash,
    ) -> impl Future<Output = Result<Pending, FactorError>> + Send;

    fn close(&self, digest: SecretHash) -> impl Future<Output = Result<(), FactorError>> + Send;
}

pub trait SessionStore {
    fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> impl Future<Output = Result<SessionToken, SessionError>> + Send;

    fn resolve(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<Session, SessionError>> + Send;

    fn revoke(&self, token: &SessionToken)
    -> impl Future<Output = Result<(), SessionError>> + Send;
}

impl<T: SessionStore + Sync> SessionStore for &T {
    fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> impl Future<Output = Result<SessionToken, SessionError>> + Send {
        (*self).issue(account, org, from)
    }

    fn resolve(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<Session, SessionError>> + Send {
        (*self).resolve(token)
    }

    fn revoke(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<(), SessionError>> + Send {
        (*self).revoke(token)
    }
}

impl<T: SessionStore + Send + Sync> SessionStore for std::sync::Arc<T> {
    fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> impl Future<Output = Result<SessionToken, SessionError>> + Send {
        self.as_ref().issue(account, org, from)
    }

    fn resolve(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<Session, SessionError>> + Send {
        self.as_ref().resolve(token)
    }

    fn revoke(
        &self,
        token: &SessionToken,
    ) -> impl Future<Output = Result<(), SessionError>> + Send {
        self.as_ref().revoke(token)
    }
}

pub trait SessionSwitch {
    fn switch(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;
}

pub trait SessionRoll {
    fn live(
        &self,
        account: AccountId,
        current: &SessionToken,
    ) -> impl Future<Output = Result<Vec<LiveSession>, SessionError>> + Send;

    fn revoke_one(
        &self,
        account: AccountId,
        id: SessionId,
    ) -> impl Future<Output = Result<bool, SessionError>> + Send;

    fn revoke_others(
        &self,
        account: AccountId,
        current: &SessionToken,
    ) -> impl Future<Output = Result<u64, SessionError>> + Send;
}

pub trait PasswordHasher {
    fn hash(
        &self,
        password: &Password,
    ) -> impl Future<Output = Result<PasswordHash, HashError>> + Send;

    fn check(
        &self,
        password: &Password,
        against: &PasswordHash,
    ) -> impl Future<Output = Result<PasswordCheck, HashError>> + Send;

    fn decoy(&self) -> &PasswordHash;
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

pub trait Authorizer {
    fn begin(&self) -> Result<(String, PendingFlow), AuthError>;
}

impl<T: Authorizer> Authorizer for &T {
    fn begin(&self) -> Result<(String, PendingFlow), AuthError> {
        (*self).begin()
    }
}

impl<T: Authorizer> Authorizer for std::sync::Arc<T> {
    fn begin(&self) -> Result<(String, PendingFlow), AuthError> {
        self.as_ref().begin()
    }
}

pub trait OidcFlows {
    fn begin(&self, flow: &PendingFlow) -> impl Future<Output = Result<(), FlowError>> + Send;

    fn take(&self, state: &str) -> impl Future<Output = Result<PendingFlow, FlowError>> + Send;
}

impl<T: OidcFlows + Sync> OidcFlows for &T {
    fn begin(&self, flow: &PendingFlow) -> impl Future<Output = Result<(), FlowError>> + Send {
        (*self).begin(flow)
    }

    fn take(&self, state: &str) -> impl Future<Output = Result<PendingFlow, FlowError>> + Send {
        (*self).take(state)
    }
}

impl<T: OidcFlows + Send + Sync> OidcFlows for std::sync::Arc<T> {
    fn begin(&self, flow: &PendingFlow) -> impl Future<Output = Result<(), FlowError>> + Send {
        self.as_ref().begin(flow)
    }

    fn take(&self, state: &str) -> impl Future<Output = Result<PendingFlow, FlowError>> + Send {
        self.as_ref().take(state)
    }
}
