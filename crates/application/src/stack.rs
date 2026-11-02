use ferristrace_domain::{
    AckKey, AlertMailer, AuthProvider, Authorizer, Clock, ClusterReader, DeployFeed,
    ESCALATE_AFTER_SECS, IssueReader, LinkKey, LogReader, LogSink, Mailer, MetricReader,
    MetricSink, PasswordHasher, Photos, PlacementReader, ProvisionPolicy, QuotaPolicy,
    ReleaseReader, SpanPages, SpanReader, SpanSink, StatusMailer, TenantDirectory, UsageMeter,
    UsageReader, WebhookSender,
};

use crate::registry::UnitOfWork;

pub trait Stack: Send + Sync + 'static {
    type Repos: UnitOfWork;
    type Clock: Clock + Send + Sync + 'static;
    type Hasher: PasswordHasher + Send + Sync + 'static;
    type Mailer: Mailer + AlertMailer + StatusMailer + Send + Sync + 'static;
    type Photos: Photos + Send + Sync + 'static;
    type SpanStore: SpanReader + SpanPages + PlacementReader + Send + Sync + 'static;
    type IssueStore: IssueReader + ReleaseReader + DeployFeed + Send + Sync + 'static;
    type LogStore: LogReader + Send + Sync + 'static;
    type MetricStore: MetricReader + ClusterReader + Send + Sync + 'static;
    type Meter: UsageMeter + UsageReader + Send + Sync + 'static;
    type Oidc: Authorizer + AuthProvider + Send + Sync + 'static;
    type Sink: SpanSink + LogSink + MetricSink + Send + Sync + 'static;
    type Quota: QuotaPolicy + Send + Sync + 'static;
    type Directory: TenantDirectory + Send + Sync + 'static;
    type Webhooks: WebhookSender + Send + Sync + 'static;
}

#[derive(Debug)]
pub struct Adapters<S: Stack> {
    pub clock: S::Clock,
    pub hasher: S::Hasher,
    pub mailer: S::Mailer,
    pub webhooks: S::Webhooks,
    pub photos: S::Photos,
    pub meter: S::Meter,
    pub span_store: Option<S::SpanStore>,
    pub issue_store: Option<S::IssueStore>,
    pub log_store: Option<S::LogStore>,
    pub metric_store: Option<S::MetricStore>,
    pub oidc: Option<S::Oidc>,
    pub provision: ProvisionPolicy,
}

#[derive(Debug)]
pub struct Ferristrace<S: Stack> {
    pub(crate) repos: S::Repos,
    pub(crate) clock: S::Clock,
    pub(crate) hasher: S::Hasher,
    pub(crate) mailer: S::Mailer,
    pub(crate) webhooks: S::Webhooks,
    pub(crate) photos: S::Photos,
    pub(crate) meter: S::Meter,
    pub(crate) span_store: Option<S::SpanStore>,
    pub(crate) issue_store: Option<S::IssueStore>,
    pub(crate) log_store: Option<S::LogStore>,
    pub(crate) metric_store: Option<S::MetricStore>,
    pub(crate) oidc: Option<S::Oidc>,
    pub(crate) provision: ProvisionPolicy,
    pub(crate) ack_key: Option<AckKey>,
    pub(crate) status_key: Option<LinkKey>,
    pub(crate) escalate_after_secs: u64,
}

impl<S: Stack> Ferristrace<S> {
    #[must_use]
    pub fn with_ack_key(mut self, key: Option<AckKey>) -> Self {
        self.ack_key = key;
        self
    }

    #[must_use]
    pub fn with_status_key(mut self, key: Option<LinkKey>) -> Self {
        self.status_key = key;
        self
    }

    #[must_use]
    pub fn escalating_after(mut self, secs: u64) -> Self {
        self.escalate_after_secs = secs;
        self
    }

    pub fn new(repos: S::Repos, adapters: Adapters<S>) -> Self {
        Self {
            repos,
            clock: adapters.clock,
            hasher: adapters.hasher,
            mailer: adapters.mailer,
            webhooks: adapters.webhooks,
            photos: adapters.photos,
            meter: adapters.meter,
            span_store: adapters.span_store,
            issue_store: adapters.issue_store,
            log_store: adapters.log_store,
            metric_store: adapters.metric_store,
            oidc: adapters.oidc,
            provision: adapters.provision,
            ack_key: None,
            status_key: None,
            escalate_after_secs: ESCALATE_AFTER_SECS,
        }
    }
}
