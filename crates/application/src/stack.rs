use ferristrace_domain::{
    AuthProvider, Authorizer, Clock, IssueReader, LogReader, LogSink, Mailer, PasswordHasher,
    Photos, ProvisionPolicy, QuotaPolicy, SpanPages, SpanReader, SpanSink, TenantDirectory,
    UsageMeter, UsageReader,
};

use crate::registry::UnitOfWork;

pub trait Stack: Send + Sync + 'static {
    type Repos: UnitOfWork;
    type Clock: Clock + Send + Sync + 'static;
    type Hasher: PasswordHasher + Send + Sync + 'static;
    type Mailer: Mailer + Send + Sync + 'static;
    type Photos: Photos + Send + Sync + 'static;
    type SpanStore: SpanReader + SpanPages + Send + Sync + 'static;
    type IssueStore: IssueReader + Send + Sync + 'static;
    type LogStore: LogReader + Send + Sync + 'static;
    type Meter: UsageMeter + UsageReader + Send + Sync + 'static;
    type Oidc: Authorizer + AuthProvider + Send + Sync + 'static;
    type Sink: SpanSink + LogSink + Send + Sync + 'static;
    type Quota: QuotaPolicy + Send + Sync + 'static;
    type Directory: TenantDirectory + Send + Sync + 'static;
}

#[derive(Debug)]
pub struct Adapters<S: Stack> {
    pub clock: S::Clock,
    pub hasher: S::Hasher,
    pub mailer: S::Mailer,
    pub photos: S::Photos,
    pub meter: S::Meter,
    pub span_store: Option<S::SpanStore>,
    pub issue_store: Option<S::IssueStore>,
    pub log_store: Option<S::LogStore>,
    pub oidc: Option<S::Oidc>,
    pub provision: ProvisionPolicy,
}

#[derive(Debug)]
pub struct Ferristrace<S: Stack> {
    pub(crate) repos: S::Repos,
    pub(crate) clock: S::Clock,
    pub(crate) hasher: S::Hasher,
    pub(crate) mailer: S::Mailer,
    pub(crate) photos: S::Photos,
    pub(crate) meter: S::Meter,
    pub(crate) span_store: Option<S::SpanStore>,
    pub(crate) issue_store: Option<S::IssueStore>,
    pub(crate) log_store: Option<S::LogStore>,
    pub(crate) oidc: Option<S::Oidc>,
    pub(crate) provision: ProvisionPolicy,
}

impl<S: Stack> Ferristrace<S> {
    pub fn new(repos: S::Repos, adapters: Adapters<S>) -> Self {
        Self {
            repos,
            clock: adapters.clock,
            hasher: adapters.hasher,
            mailer: adapters.mailer,
            photos: adapters.photos,
            meter: adapters.meter,
            span_store: adapters.span_store,
            issue_store: adapters.issue_store,
            log_store: adapters.log_store,
            oidc: adapters.oidc,
            provision: adapters.provision,
        }
    }
}
