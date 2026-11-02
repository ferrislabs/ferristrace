use std::sync::Arc;

use ferristrace_application::Stack;
use ferristrace_application::registry::Repositories;
use ferristrace_clickhouse::{ClickhouseIssues, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::telemetry::{
    BillingHour, CounterUnreachable, HourTotal, HourlyCounters, StaticQuota,
};
use ferristrace_domain::{
    DecompressedBytes, Email, IngestKey, Mailer, NoPhotos, Photo, PhotoError, Photos, Pictured,
    Portrait, ResetError, ResetToken, Scope, SinkError, Span, SpanSink, TenantDirectory, TenantId,
    UnknownKey, UsageMeter, UsageReader,
};
use ferristrace_infra::{
    Argon2Hasher, MemoryMeter, MemorySink, OidcProvider, S3Photos, SmtpMailer, StaticDirectory,
    SystemClock,
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
    type Meter = Arc<Meter>;
    type Oidc = OidcProvider;
    type Sink = Arc<Sink>;
    type Quota = StaticQuota<Arc<Meter>>;
    type Directory = Directory;
}

const _: fn() = || {
    fn holds<R: Repositories>() {}
    holds::<<Production as Stack>::Repos>();
};

#[derive(Debug)]
pub(crate) enum Sink {
    Memory(MemorySink),
    Spans(Box<ClickhouseSink<SystemClock>>),
}

impl SpanSink for Sink {
    async fn accept(&self, scope: Scope, spans: Vec<Span>) -> Result<(), SinkError> {
        match self {
            Self::Memory(sink) => sink.accept(scope, spans).await,
            Self::Spans(sink) => sink.accept(scope, spans).await,
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
