use core::marker::PhantomData;
use std::sync::Arc;

use ferristrace_api::AppState;
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    AccountDirectory, AccountId, AccountName, Accounts, AuthError, Membership, Memberships, Photos,
    Portrait, ProvisionPolicy, ResolvedIdentity, Session, SessionError, SessionStore, SessionToken,
    StoreError, UserTokens, WhoamiError,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct Nowhere;

impl SessionStore for Nowhere {
    async fn resolve(&self, _token: &SessionToken) -> Result<Session, SessionError> {
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

#[derive(Debug, Clone)]
pub struct Sessioned<T>(T);

impl<T: SessionStore + Send + Sync> SessionStore for Sessioned<T> {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        self.0.resolve(token).await
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

#[derive(Debug)]
pub struct Fakes<S, M, A> {
    pub sessions: S,
    pub memberships: M,
    pub accounts: A,
}

impl Fakes<Nowhere, Nowhere, Nowhere> {
    pub fn nowhere() -> Self {
        Self {
            sessions: Nowhere,
            memberships: Nowhere,
            accounts: Nowhere,
        }
    }
}

impl<S, M, A> Untransacted for Fakes<S, M, A>
where
    S: Send + Sync + 'static,
    M: Send + Sync + 'static,
    A: Send + Sync + 'static,
{
}

impl<S, M, A> RepoFor<domain::Sessions> for Fakes<S, M, A>
where
    S: SessionStore + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = Sessioned<S>;

    fn build(&self, (): &()) -> Self::Repo {
        Sessioned(self.sessions.clone())
    }
}

impl<S, M, A> RepoFor<domain::Memberships> for Fakes<S, M, A>
where
    M: Memberships + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = M;

    fn build(&self, (): &()) -> Self::Repo {
        self.memberships.clone()
    }
}

impl<S, M, A> RepoFor<domain::Accounts> for Fakes<S, M, A>
where
    A: Accounts + AccountDirectory + Clone + Send + Sync + 'static,
    Self: Untransacted,
{
    type Repo = A;

    fn build(&self, (): &()) -> Self::Repo {
        self.accounts.clone()
    }
}

type Unowned<T> = PhantomData<fn() -> T>;

#[derive(Debug)]
pub struct Wired<R, M, P, Z>(Unowned<(R, M, P, Z)>);

impl<R, M, P, Z> Stack for Wired<R, M, P, Z>
where
    R: ferristrace_application::registry::UnitOfWork,
    M: ferristrace_domain::AlertMailer + ferristrace_domain::StatusMailer + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
    Z: UserTokens + Send + Sync + 'static,
{
    type Repos = R;
    type Clock = Inert;
    type Mailer = M;
    type Photos = P;
    type SpanStore = Inert;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Users = Z;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

pub fn state<R, M, P, Z>(
    repos: R,
    mailer: M,
    photos: P,
    users: Option<Z>,
) -> AppState<Wired<R, M, P, Z>>
where
    R: ferristrace_application::registry::UnitOfWork,
    M: ferristrace_domain::AlertMailer + ferristrace_domain::StatusMailer + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
    Z: UserTokens + Send + Sync + 'static,
{
    let adapters = Adapters {
        clock: Inert::default(),
        mailer,
        webhooks: Inert::default(),
        photos,
        meter: Inert::default(),
        span_store: None,
        issue_store: None,
        log_store: None,
        metric_store: None,
        users,
    };
    AppState::new(Arc::new(Ferristrace::new(repos, adapters)))
}
