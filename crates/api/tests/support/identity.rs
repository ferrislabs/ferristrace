use core::marker::PhantomData;
use std::sync::Arc;

use ferristrace_api::{AppState, SessionCookie};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountDirectory, AccountId, AccountName, Accounts, Agent, AuthError, AuthProvider, Authorizer,
    Email, FactorError, FactorKind, FactorStore, FlowError, Gate, Kept, LiveSession, Mailer,
    Membership, Memberships, OidcFlows, PasswordHash, PasswordHasher, Pending, PendingFlow, Photos,
    Portrait, ProvisionPolicy, ResetError, ResetStore, ResolvedIdentity, SecretHash, Session,
    SessionError, SessionId, SessionRoll, SessionStore, SessionToken, SignInProof, Step,
    StoreError, TenantId, TotpSecret, WhoamiError,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct Nowhere;

impl SessionStore for Nowhere {
    async fn issue(
        &self,
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Err(SessionError::StoreUnreachable)
    }

    async fn resolve(&self, _token: &SessionToken) -> Result<Session, SessionError> {
        Err(SessionError::StoreUnreachable)
    }

    async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
        Err(SessionError::StoreUnreachable)
    }
}

impl Memberships for Nowhere {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Err(WhoamiError::StoreUnreachable)
    }
}

impl Accounts for Nowhere {
    async fn rename(
        &self,
        _account: AccountId,
        _name: Option<&AccountName>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn portrait_of(&self, _account: AccountId) -> Result<Option<Portrait>, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn set_portrait(
        &self,
        _account: AccountId,
        _portrait: Option<&Portrait>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unreachable)
    }
}

impl AccountDirectory for Nowhere {
    async fn provision(
        &self,
        _identity: &ResolvedIdentity,
        _policy: ProvisionPolicy,
    ) -> Result<AccountId, AuthError> {
        Err(AuthError::ProviderUnreachable)
    }
}

impl Gate for Nowhere {
    async fn offered(&self, _account: AccountId) -> Result<Vec<FactorKind>, FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn open(&self, _account: AccountId, _digest: SecretHash) -> Result<(), FactorError> {
        Err(FactorError::StoreUnreachable)
    }
}

impl ResetStore for Nowhere {
    async fn account_for(&self, _email: &Email) -> Result<Option<AccountId>, ResetError> {
        Err(ResetError::StoreUnreachable)
    }

    async fn begin(&self, _account: AccountId, _digest: SecretHash) -> Result<(), ResetError> {
        Err(ResetError::StoreUnreachable)
    }

    async fn take(&self, _digest: SecretHash) -> Result<AccountId, ResetError> {
        Err(ResetError::StoreUnreachable)
    }

    async fn set_password(
        &self,
        _account: AccountId,
        _hash: &PasswordHash,
    ) -> Result<(), ResetError> {
        Err(ResetError::StoreUnreachable)
    }

    async fn revoke_sessions(&self, _account: AccountId) -> Result<u64, ResetError> {
        Err(ResetError::StoreUnreachable)
    }
}

impl OidcFlows for Nowhere {
    async fn begin(&self, _flow: &PendingFlow) -> Result<(), FlowError> {
        Err(FlowError::StoreUnreachable)
    }

    async fn take(&self, _state: &str) -> Result<PendingFlow, FlowError> {
        Err(FlowError::StoreUnreachable)
    }
}

impl AuthProvider for Nowhere {
    async fn sign_in(&self, _proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        Err(AuthError::ProviderUnreachable)
    }
}

#[derive(Debug, Clone)]
pub struct Sessioned<T>(T);

impl<T: SessionStore + Send + Sync> SessionStore for Sessioned<T> {
    async fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        self.0.issue(account, org, from).await
    }

    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        self.0.resolve(token).await
    }

    async fn revoke(&self, token: &SessionToken) -> Result<(), SessionError> {
        self.0.revoke(token).await
    }
}

impl<T: Send + Sync> SessionRoll for Sessioned<T> {
    async fn live(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<Vec<LiveSession>, SessionError> {
        Err(SessionError::StoreUnreachable)
    }

    async fn revoke_one(&self, _account: AccountId, _id: SessionId) -> Result<bool, SessionError> {
        Err(SessionError::StoreUnreachable)
    }

    async fn revoke_others(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<u64, SessionError> {
        Err(SessionError::StoreUnreachable)
    }
}

#[derive(Debug, Clone)]
pub struct Directed<T>(pub T);

impl<T: AccountDirectory + Send + Sync> AccountDirectory for Directed<T> {
    async fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> Result<AccountId, AuthError> {
        self.0.provision(identity, policy).await
    }
}

impl<T: Send + Sync> Accounts for Directed<T> {
    async fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> Result<(), StoreError> {
        Nowhere.rename(account, name).await
    }

    async fn portrait_of(&self, account: AccountId) -> Result<Option<Portrait>, StoreError> {
        Nowhere.portrait_of(account).await
    }

    async fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> Result<(), StoreError> {
        Nowhere.set_portrait(account, portrait).await
    }
}

#[derive(Debug, Clone)]
pub struct Profiled<T>(pub T);

impl<T: Accounts + Send + Sync> Accounts for Profiled<T> {
    async fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> Result<(), StoreError> {
        self.0.rename(account, name).await
    }

    async fn portrait_of(&self, account: AccountId) -> Result<Option<Portrait>, StoreError> {
        self.0.portrait_of(account).await
    }

    async fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> Result<(), StoreError> {
        self.0.set_portrait(account, portrait).await
    }
}

impl<T: Send + Sync> AccountDirectory for Profiled<T> {
    async fn provision(
        &self,
        identity: &ResolvedIdentity,
        policy: ProvisionPolicy,
    ) -> Result<AccountId, AuthError> {
        Nowhere.provision(identity, policy).await
    }
}

#[derive(Debug, Clone)]
pub struct Gated<T>(T);

impl<T: Gate + Send + Sync> Gate for Gated<T> {
    async fn offered(&self, account: AccountId) -> Result<Vec<FactorKind>, FactorError> {
        self.0.offered(account).await
    }

    async fn open(&self, account: AccountId, digest: SecretHash) -> Result<(), FactorError> {
        self.0.open(account, digest).await
    }
}

impl<T: Send + Sync> FactorStore for Gated<T> {
    async fn live(&self, _account: AccountId) -> Result<Vec<FactorKind>, FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn keep(
        &self,
        _account: AccountId,
        _kind: FactorKind,
        _secret: &TotpSecret,
    ) -> Result<(), FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn kept(
        &self,
        _account: AccountId,
        _kind: FactorKind,
    ) -> Result<Option<Kept>, FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn confirm(&self, _account: AccountId, _kind: FactorKind) -> Result<(), FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn forget(&self, _account: AccountId, _kind: FactorKind) -> Result<bool, FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn spent_step(
        &self,
        _account: AccountId,
        _kind: FactorKind,
        _step: Step,
    ) -> Result<(), FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn keep_recovery(
        &self,
        _account: AccountId,
        _digests: &[SecretHash],
    ) -> Result<(), FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn spend_recovery(
        &self,
        _account: AccountId,
        _digest: SecretHash,
    ) -> Result<bool, FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn recovery_left(&self, _account: AccountId) -> Result<u32, FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn answered(&self, _digest: SecretHash) -> Result<Pending, FactorError> {
        Err(FactorError::StoreUnreachable)
    }

    async fn close(&self, _digest: SecretHash) -> Result<(), FactorError> {
        Err(FactorError::StoreUnreachable)
    }
}

#[derive(Debug)]
pub struct Fakes<S, M, A, F, R, W, L> {
    pub sessions: S,
    pub memberships: M,
    pub accounts: A,
    pub gate: F,
    pub resets: R,
    pub flows: W,
    pub local: L,
}

impl Fakes<Nowhere, Nowhere, Nowhere, Nowhere, Nowhere, Nowhere, Nowhere> {
    pub fn nowhere() -> Self {
        Self {
            sessions: Nowhere,
            memberships: Nowhere,
            accounts: Nowhere,
            gate: Nowhere,
            resets: Nowhere,
            flows: Nowhere,
            local: Nowhere,
        }
    }
}

impl<S, M, A, F, R, W, L> Untransacted for Fakes<S, M, A, F, R, W, L>
where
    S: Send + Sync + 'static,
    M: Send + Sync + 'static,
    A: Send + Sync + 'static,
    F: Send + Sync + 'static,
    R: Send + Sync + 'static,
    W: Send + Sync + 'static,
    L: Send + Sync + 'static,
{
}

impl<S, M, A, F, R, W, L> RepoFor<domain::Sessions> for Fakes<S, M, A, F, R, W, L>
where
    S: SessionStore + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = Sessioned<S>;

    fn build(&self, (): &()) -> Self::Repo {
        Sessioned(self.sessions.clone())
    }
}

impl<S, M, A, F, R, W, L> RepoFor<domain::Memberships> for Fakes<S, M, A, F, R, W, L>
where
    M: Memberships + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = M;

    fn build(&self, (): &()) -> Self::Repo {
        self.memberships.clone()
    }
}

impl<S, M, A, F, R, W, L> RepoFor<domain::Accounts> for Fakes<S, M, A, F, R, W, L>
where
    A: Accounts + AccountDirectory + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = A;

    fn build(&self, (): &()) -> Self::Repo {
        self.accounts.clone()
    }
}

impl<S, M, A, F, R, W, L> RepoFor<domain::Factors> for Fakes<S, M, A, F, R, W, L>
where
    F: Gate + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = Gated<F>;

    fn build(&self, (): &()) -> Self::Repo {
        Gated(self.gate.clone())
    }
}

impl<S, M, A, F, R, W, L> RepoFor<domain::Resets> for Fakes<S, M, A, F, R, W, L>
where
    R: ResetStore + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = R;

    fn build(&self, (): &()) -> Self::Repo {
        self.resets.clone()
    }
}

impl<S, M, A, F, R, W, L> RepoFor<domain::Flows> for Fakes<S, M, A, F, R, W, L>
where
    W: OidcFlows + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = W;

    fn build(&self, (): &()) -> Self::Repo {
        self.flows.clone()
    }
}

impl<S, M, A, F, R, W, L> RepoFor<domain::LocalPasswords> for Fakes<S, M, A, F, R, W, L>
where
    L: AuthProvider + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = L;

    fn build(&self, (): &()) -> Self::Repo {
        self.local.clone()
    }
}

type Unowned<T> = PhantomData<fn() -> T>;

#[derive(Debug)]
pub struct Wired<R, H, M, P, Z>(Unowned<(R, H, M, P, Z)>);

impl<R, H, M, P, Z> Stack for Wired<R, H, M, P, Z>
where
    R: ferristrace_application::registry::UnitOfWork,
    H: PasswordHasher + Send + Sync + 'static,
    M: Mailer + ferristrace_domain::AlertMailer + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
    Z: Authorizer + AuthProvider + Send + Sync + 'static,
{
    type Repos = R;
    type Clock = Inert;
    type Hasher = H;
    type Mailer = M;
    type Photos = P;
    type SpanStore = Inert;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Oidc = Z;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

pub fn state<R, H, M, P, Z>(
    repos: R,
    hasher: H,
    mailer: M,
    photos: P,
    oidc: Option<Z>,
    cookie: SessionCookie,
) -> AppState<Wired<R, H, M, P, Z>>
where
    R: ferristrace_application::registry::UnitOfWork,
    H: PasswordHasher + Send + Sync + 'static,
    M: Mailer + ferristrace_domain::AlertMailer + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
    Z: Authorizer + AuthProvider + Send + Sync + 'static,
{
    let adapters = Adapters {
        clock: Inert::default(),
        hasher,
        mailer,
        webhooks: Inert::default(),
        photos,
        meter: Inert::default(),
        span_store: None,
        issue_store: None,
        log_store: None,
        metric_store: None,
        oidc,
        provision: ProvisionPolicy::default(),
    };
    AppState::new(Arc::new(Ferristrace::new(repos, adapters)), cookie)
}
