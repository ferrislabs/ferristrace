use std::sync::Arc;

use ferristrace_application::Stack;
use ferristrace_application::registry::Repositories;
use ferristrace_clickhouse::{
    ClickhouseIssues, ClickhouseLogReader, ClickhouseLogSink, ClickhouseMetricReader,
    ClickhouseMetricSink, ClickhouseReader, ClickhouseSink,
};
use ferristrace_domain::telemetry::{
    BillingHour, CounterUnreachable, HourTotal, HourlyCounters, StaticQuota,
};
use ferristrace_domain::{
    AlertMailer, DecompressedBytes, DeliveryError, Dispatch, Email, EmailList, IngestKey, Letter,
    LogRecord, LogSink, Mailer, Metric, MetricSink, NoPhotos, PageDispatch, Photo, PhotoError,
    Photos, Pictured, Portrait, ResetError, ResetToken, Scope, SinkError, Span, SpanSink,
    StatusMailer, TenantDirectory, TenantId, UnknownKey, UsageMeter, UsageReader,
};
use ferristrace_infra::{
    Argon2Hasher, HttpWebhooks, MemoryMeter, MemorySink, OidcProvider, S3Photos, SmtpMailer,
    StaticDirectory, SystemClock,
};
use ferristrace_postgres::{Postgres, PostgresDirectory};
use ferristrace_redis::RedisMeter;

#[derive(Debug)]
pub(crate) struct Production;

impl Stack for Production {
    type Repos = Postgres<SystemClock>;
    type Clock = SystemClock;
    type Hasher = Argon2Hasher;
    type Mailer = Relay;
    type Photos = Gallery;
    type SpanStore = ClickhouseReader;
    type IssueStore = ClickhouseIssues;
    type LogStore = ClickhouseLogReader;
    type MetricStore = ClickhouseMetricReader;
    type Meter = Arc<Meter>;
    type Oidc = OidcProvider;
    type Sink = Arc<Sink>;
    type Quota = StaticQuota<Arc<Meter>>;
    type Directory = Directory;
    type Webhooks = HttpWebhooks;
}

const _: fn() = || {
    fn holds<R: Repositories>() {}
    holds::<<Production as Stack>::Repos>();
};

#[derive(Debug)]
pub(crate) struct Stored {
    pub(crate) spans: ClickhouseSink<SystemClock>,
    pub(crate) logs: ClickhouseLogSink<SystemClock>,
    pub(crate) metrics: ClickhouseMetricSink<SystemClock>,
}

#[derive(Debug)]
pub(crate) enum Sink {
    Memory(MemorySink),
    Store(Box<Stored>),
}

impl SpanSink for Sink {
    async fn accept(&self, scope: Scope, spans: Vec<Span>) -> Result<(), SinkError> {
        match self {
            Self::Memory(sink) => sink.accept(scope, spans).await,
            Self::Store(store) => store.spans.accept(scope, spans).await,
        }
    }
}

impl LogSink for Sink {
    async fn accept_logs(&self, scope: Scope, records: Vec<LogRecord>) -> Result<(), SinkError> {
        match self {
            Self::Memory(sink) => sink.accept_logs(scope, records).await,
            Self::Store(store) => store.logs.accept_logs(scope, records).await,
        }
    }
}

impl MetricSink for Sink {
    async fn accept_metrics(&self, scope: Scope, metrics: Vec<Metric>) -> Result<(), SinkError> {
        match self {
            Self::Memory(sink) => sink.accept_metrics(scope, metrics).await,
            Self::Store(store) => store.metrics.accept_metrics(scope, metrics).await,
        }
    }
}

#[derive(Debug)]
pub(crate) enum Relay {
    Smtp(Box<SmtpMailer>),
    Unreachable,
}

impl Mailer for Relay {
    async fn send_password_reset(&self, to: &Email, token: &ResetToken) -> Result<(), ResetError> {
        match self {
            Self::Smtp(mailer) => mailer.send_password_reset(to, token).await,
            Self::Unreachable => {
                tracing::warn!("a reset link was asked for but no mail relay is configured");
                Err(ResetError::Undeliverable)
            }
        }
    }
}

impl AlertMailer for Relay {
    async fn send_alert(&self, to: &EmailList, dispatch: &Dispatch) -> Result<(), DeliveryError> {
        match self {
            Self::Smtp(mailer) => mailer.send_alert(to, dispatch).await,
            Self::Unreachable => {
                tracing::warn!("an alert was to be mailed but no mail relay is configured");
                Err(DeliveryError::new("no mail relay is configured"))
            }
        }
    }

    async fn send_page(&self, to: &Email, dispatch: &PageDispatch) -> Result<(), DeliveryError> {
        match self {
            Self::Smtp(mailer) => mailer.send_page(to, dispatch).await,
            Self::Unreachable => {
                tracing::warn!("a page was to be mailed but no mail relay is configured");
                Err(DeliveryError::new("no mail relay is configured"))
            }
        }
    }
}

impl StatusMailer for Relay {
    async fn send_status(&self, to: &Email, letter: &Letter) -> Result<(), DeliveryError> {
        match self {
            Self::Smtp(mailer) => mailer.send_status(to, letter).await,
            Self::Unreachable => {
                tracing::warn!(
                    "a status page email was to be sent but no mail relay is configured"
                );
                Err(DeliveryError::new("no mail relay is configured"))
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum Gallery {
    Objects(Box<S3Photos<SystemClock>>),
    Nowhere,
}

impl Photos for Gallery {
    async fn put(&self, of: Pictured, photo: &Photo) -> Result<Portrait, PhotoError> {
        match self {
            Self::Objects(store) => store.put(of, photo).await,
            Self::Nowhere => {
                tracing::warn!("a photo was offered but no object store is configured");
                NoPhotos.put(of, photo).await
            }
        }
    }

    async fn fetch(
        &self,
        of: Pictured,
        portrait: &Portrait,
    ) -> Result<Option<Vec<u8>>, PhotoError> {
        match self {
            Self::Objects(store) => store.fetch(of, portrait).await,
            Self::Nowhere => NoPhotos.fetch(of, portrait).await,
        }
    }

    async fn remove(&self, of: Pictured, portrait: &Portrait) -> Result<(), PhotoError> {
        match self {
            Self::Objects(store) => store.remove(of, portrait).await,
            Self::Nowhere => NoPhotos.remove(of, portrait).await,
        }
    }
}

#[derive(Debug)]
pub(crate) enum Meter {
    Memory(MemoryMeter),
    Redis(RedisMeter<SystemClock>),
}

impl UsageMeter for Meter {
    async fn record_ingest(&self, tenant: TenantId, decompressed: DecompressedBytes) {
        match self {
            Self::Memory(meter) => meter.record_ingest(tenant, decompressed).await,
            Self::Redis(meter) => meter.record_ingest(tenant, decompressed).await,
        }
    }
}

impl UsageReader for Meter {
    async fn used(&self, tenant: TenantId) -> DecompressedBytes {
        match self {
            Self::Memory(meter) => meter.used(tenant).await,
            Self::Redis(meter) => meter.used(tenant).await,
        }
    }
}

impl HourlyCounters for Meter {
    async fn finished(&self, hour: BillingHour) -> Result<Vec<HourTotal>, CounterUnreachable> {
        match self {
            Self::Memory(_) => Ok(Vec::new()),
            Self::Redis(meter) => meter.finished(hour).await,
        }
    }

    async fn discard(&self, total: HourTotal) -> Result<(), CounterUnreachable> {
        match self {
            Self::Memory(_) => Ok(()),
            Self::Redis(meter) => meter.discard(total).await,
        }
    }

    async fn seed(
        &self,
        tenant: TenantId,
        total: DecompressedBytes,
    ) -> Result<bool, CounterUnreachable> {
        match self {
            Self::Memory(_) => Ok(false),
            Self::Redis(meter) => meter.seed(tenant, total).await,
        }
    }
}

#[derive(Debug)]
pub(crate) enum Directory {
    Nothing(StaticDirectory),
    ControlPlane(PostgresDirectory<SystemClock>),
}

impl TenantDirectory for Directory {
    async fn resolve(&self, key: &IngestKey) -> Result<Scope, UnknownKey> {
        match self {
            Self::Nothing(directory) => directory.resolve(key).await,
            Self::ControlPlane(directory) => directory.resolve(key).await,
        }
    }
}
