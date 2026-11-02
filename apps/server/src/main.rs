mod adapters;
mod cli;
mod config;
mod routes;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use clap::Parser;
use ferristrace_api::{AppState, health_router, router, trace_server, web_router};
use ferristrace_application::{Adapters, Ferristrace, Ingestion};
use ferristrace_clickhouse::{
    ClickhouseIssues, ClickhouseReader, SpanStoreSettings, open_span_store,
};
use ferristrace_domain::{Clock, ProvisionPolicy, TenantId};
use ferristrace_infra::{
    Allowance, Argon2Hasher, MailSettings, MemoryMeter, MemorySink, OidcProvider, OidcSettings,
    PhotoStoreSettings, S3Photos, SecretSeal, SmtpMailer, StaticDirectory, StaticQuota,
    SystemClock,
};
use ferristrace_postgres::{
    CONTROL_PLANE, INVITE_WINDOW, Postgres, PostgresDirectory, load_allowances,
};
use ferristrace_redis::RedisMeter;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tonic::transport::Server;
use tracing_subscriber::EnvFilter;

use crate::adapters::{Directory, Gallery, Meter, Production, Relay, Sink};
use crate::cli::{Cli, Command, health, run_command};
use crate::config::Config;
use crate::routes::{Held, State, control_routes, cookie_of};

const SECONDS_PER_HOUR: u64 = 3600;
const FLUSH_FALLBACK: Duration = Duration::from_secs(5);
const MIN_FLUSH_WAIT: Duration = Duration::from_millis(100);

#[derive(Default)]
struct Optional {
    span_store: Option<ClickhouseReader>,
    issue_store: Option<ClickhouseIssues>,
    oidc: Option<OidcProvider>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let config = config::load().context("reading the configuration")?;
    match Cli::parse().command {
        Some(Command::Health) => return health(&config).await,
        Some(command) => return provision(command, &config).await,
        None => {}
    }
    let pool = control_plane_pool(&config).await?;
    let allowances = allowances_of(&config, pool.as_ref()).await?;
    let meter = Arc::new(meter_of(&config, pool.as_ref()).await?);
    if let (Some(pool), Meter::Redis(_)) = (pool.as_ref(), meter.as_ref()) {
        tokio::spawn(roll_up_hourly(Arc::clone(&meter), pool.clone()));
    }
    let (sink, reader, grouping) = spans_of(&config).await?;
    let held = Held {
        explore: reader.is_some(),
        issues: grouping.is_some(),
    };
    let sink = Arc::new(sink);
    tokio::spawn(flush_spans(Arc::clone(&sink)));
    let gallery = gallery_of(&config)?;
    let state = match pool.as_ref() {
        Some(pool) => {
            let optional = Optional {
                span_store: reader,
                issue_store: grouping,
                oidc: oidc_of(&config).await?,
            };
            Some(control_plane(&config, pool, gallery, Arc::clone(&meter), optional).await?)
        }
        None => None,
    };
    let identity = control_routes(&config, state.as_ref(), held);
    let gateway = Arc::new(gateway_of(
        directory_of(pool.as_ref()),
        allowances,
        meter,
        Arc::clone(&sink),
    ));

    tracing::info!(
        grpc = %config.grpc_address,
        http = %config.http_address,
        tenants = config.tenants.len(),
        "the ingestion gateway is starting"
    );

    let http = tokio::spawn(serve_http(
        config.http_address,
        Arc::clone(&gateway),
        identity,
        web_routes(&config),
    ));
    let grpc = tokio::spawn(serve_grpc(config.grpc_address, gateway));

    let (http, grpc) = tokio::try_join!(http, grpc).context("joining the listeners")?;
    flush_once(&sink).await;
    http?;
    grpc?;
    Ok(())
}

async fn provision(command: Command, config: &Config) -> anyhow::Result<()> {
    let pool = control_plane_pool(config)
        .await?
        .context("this command needs a control plane, so set database_url")?;
    let hasher = Argon2Hasher::new().context("preparing the password hasher")?;
    let repos = Postgres::new(pool, SystemClock)
        .with_hasher(hasher.clone())
        .with_invite_window(INVITE_WINDOW);
    let adapters = adapters_of(
        hasher,
        Relay::Unreachable,
        Gallery::Nowhere,
        Arc::new(Meter::Memory(MemoryMeter::default())),
        Optional::default(),
    );
    let app = Ferristrace::<Production>::new(repos, adapters);
    run_command(command, config, &app, INVITE_WINDOW).await
}

async fn control_plane(
    config: &Config,
    pool: &PgPool,
    gallery: Gallery,
    meter: Arc<Meter>,
    optional: Optional,
) -> anyhow::Result<State> {
    let hasher = Argon2Hasher::new().context("preparing the password hasher")?;
    let seal = if config.local_password {
        seal_of(config)?
    } else {
        None
    };
    let mailer = if config.local_password {
        relay_of(config)?
    } else {
        Relay::Unreachable
    };
    let repos = Postgres::new(pool.clone(), SystemClock)
        .with_hasher(hasher.clone())
        .with_seal(seal)
        .with_invite_window(INVITE_WINDOW);
    let adapters = adapters_of(hasher, mailer, gallery, meter, optional);
    let app = Arc::new(Ferristrace::<Production>::new(repos, adapters));
    Ok(AppState::new(app, cookie_of(config)))
}

fn adapters_of(
    hasher: Argon2Hasher,
    mailer: Relay,
    photos: Gallery,
    meter: Arc<Meter>,
    optional: Optional,
) -> Adapters<Production> {
    Adapters {
        clock: SystemClock,
        hasher,
        mailer,
        photos,
        meter,
        span_store: optional.span_store,
        issue_store: optional.issue_store,
        oidc: optional.oidc,
        provision: ProvisionPolicy::default(),
    }
}

async fn control_plane_pool(config: &Config) -> anyhow::Result<Option<PgPool>> {
    let Some(url) = config.database_url.as_deref() else {
        return Ok(None);
    };
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(url)
        .await
        .context("connecting to the control plane")?;
    CONTROL_PLANE
        .run(&pool)
        .await
        .context("migrating the control plane")?;
    Ok(Some(pool))
}

async fn allowances_of(
    config: &Config,
    pool: Option<&PgPool>,
) -> anyhow::Result<BTreeMap<TenantId, Allowance>> {
    let Some(pool) = pool else {
        return Ok(config
            .tenants
            .iter()
            .map(|tenant| (TenantId::new(tenant.id), tenant.allowance.into()))
            .collect());
    };
    let loaded = load_allowances(pool)
        .await
        .context("reading the tenant allowances")?;
    tracing::info!(
        tenants = loaded.len(),
        "allowances read from the control plane"
    );
    Ok(loaded)
}

async fn meter_of(config: &Config, pool: Option<&PgPool>) -> anyhow::Result<Meter> {
    let Some(url) = config.redis_url.as_deref() else {
        return Ok(Meter::Memory(MemoryMeter::default()));
    };

    let meter = RedisMeter::connect(url, SystemClock)
        .await
        .context("connecting to the usage counter store")?;
    if let Some(pool) = pool {
        let seeded = meter
            .rehydrate(pool)
            .await
            .context("rehydrating the usage counters")?;
        tracing::info!(tenants = seeded, "usage counters rehydrated");
    }
    Ok(Meter::Redis(meter))
}

fn directory_of(pool: Option<&PgPool>) -> Directory {
    let Some(pool) = pool else {
        tracing::warn!(
            "no control plane is configured, so no ingest key can authenticate; \
             issue one with `comettrace issue-key` once database_url is set"
        );
        return Directory::Nothing(StaticDirectory::default());
    };
    Directory::ControlPlane(PostgresDirectory::new(pool.clone(), SystemClock))
}

fn gateway_of(
    directory: Directory,
    allowances: BTreeMap<TenantId, Allowance>,
    meter: Arc<Meter>,
    sink: Arc<Sink>,
) -> Ingestion<Production> {
    let quota = StaticQuota::new(Arc::clone(&meter), allowances);
    Ingestion::new(directory, quota, sink, meter)
}

async fn spans_of(
    config: &Config,
) -> anyhow::Result<(Sink, Option<ClickhouseReader>, Option<ClickhouseIssues>)> {
    let Some(spans) = config.spans.clone() else {
        tracing::warn!(
            "no span store is configured, so accepted spans are held in memory and \
             lost when this process ends, and no trace can be read back; \
             set spans.url to keep them"
        );
        return Ok((Sink::Memory(MemorySink::default()), None, None));
    };
    let settings = SpanStoreSettings::from(spans);
    let store = open_span_store(&settings, SystemClock)
        .await
        .context("opening the span store")?;
    tracing::info!(
        url = %settings.url,
        retention_days = settings.retention_days,
        "spans are written to the span store"
    );
    Ok((
        Sink::Spans(Box::new(store.sink)),
        Some(store.reader),
        Some(store.issues),
    ))
}

async fn flush_spans(sink: Arc<Sink>) {
    let Sink::Spans(spans) = sink.as_ref() else {
        return;
    };
    loop {
        let wait = spans
            .time_left()
            .await
            .unwrap_or(FLUSH_FALLBACK)
            .max(MIN_FLUSH_WAIT);
        tokio::time::sleep(wait).await;
        match spans.flush().await {
            Ok(0) => {}
            Ok(rows) => tracing::debug!(rows, "a span batch was committed on its deadline"),
            Err(error) => tracing::error!(%error, "a span batch could not be committed"),
        }
    }
}

async fn flush_once(sink: &Sink) {
    let Sink::Spans(spans) = sink else {
        return;
    };
    match spans.flush().await {
        Ok(0) => {}
        Ok(rows) => tracing::info!(rows, "the last span batch was committed before leaving"),
        Err(error) => tracing::error!(%error, "the last span batch was lost"),
    }
}

async fn oidc_of(config: &Config) -> anyhow::Result<Option<OidcProvider>> {
    let Some(settings) = &config.oidc else {
        return Ok(None);
    };
    let provider = OidcProvider::discover(OidcSettings {
        issuer: settings.issuer.clone(),
        client_id: settings.client_id.clone(),
        client_secret: settings.client_secret.clone(),
        redirect_uri: settings.redirect_uri.clone(),
    })
    .await
    .context("reaching the identity provider")?;
    Ok(Some(provider))
}

fn gallery_of(config: &Config) -> anyhow::Result<Gallery> {
    let Some(objects) = config.objects.clone() else {
        tracing::warn!(
            "no object store is configured, so nobody can keep a profile photo; set objects.endpoint, objects.bucket and its credentials to offer one"
        );
        return Ok(Gallery::Nowhere);
    };
    let bucket = objects.bucket.clone();
    let store = S3Photos::new(&PhotoStoreSettings::from(objects), SystemClock)
        .context("preparing the photo store")?;
    tracing::info!(%bucket, "profile photos are kept in an object store");
    Ok(Gallery::Objects(Box::new(store)))
}

fn seal_of(config: &Config) -> anyhow::Result<Option<SecretSeal>> {
    let Some(secrets) = &config.secrets else {
        tracing::warn!(
            "no secrets.at_rest_key is configured, so nobody can keep a second factor; mint one with `openssl rand -hex 32`"
        );
        return Ok(None);
    };
    let seal = SecretSeal::from_hex(&secrets.at_rest_key).context("reading secrets.at_rest_key")?;
    tracing::info!("second factors are sealed at rest");
    Ok(Some(seal))
}

fn relay_of(config: &Config) -> anyhow::Result<Relay> {
    let Some(mail) = &config.mail else {
        tracing::warn!(
            "no mail relay is configured, so nobody can ask for a reset link; \
             hand one over with `comettrace reset-password`"
        );
        return Ok(Relay::Unreachable);
    };
    let mailer = SmtpMailer::connect(&MailSettings {
        relay: mail.relay.clone(),
        username: mail.username.clone(),
        password: mail.password.clone(),
        from: mail.from.clone(),
        base_url: mail.base_url.clone(),
    })
    .context("preparing the mail relay")?;
    tracing::info!(relay = %mail.relay, "password resets can be sent");
    Ok(Relay::Smtp(Box::new(mailer)))
}

async fn serve_http(
    address: SocketAddr,
    gateway: Arc<Ingestion<Production>>,
    identity: Router,
    web: Router,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("binding the OTLP HTTP listener on {address}"))?;
    let served = router(gateway)
        .merge(identity)
        .merge(health_router())
        .merge(web);
    axum::serve(listener, served)
        .with_graceful_shutdown(shutdown())
        .await
        .context("serving OTLP over HTTP")
}

fn web_routes(config: &Config) -> Router {
    let Some(root) = config.web_root.as_deref().map(Path::new) else {
        return Router::new();
    };
    if !root.join("index.html").is_file() {
        tracing::warn!(
            root = %root.display(),
            "web_root carries no index.html, so nothing is served at the root"
        );
        return Router::new();
    }
    tracing::info!(root = %root.display(), "the web application is served");
    web_router(root)
}

async fn serve_grpc(
    address: SocketAddr,
    gateway: Arc<Ingestion<Production>>,
) -> anyhow::Result<()> {
    Server::builder()
        .add_service(trace_server(gateway))
        .serve_with_shutdown(address, shutdown())
        .await
        .context("serving OTLP over gRPC")
}

async fn shutdown() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "the shutdown signal handler failed");
    }
}

async fn roll_up_hourly(meter: Arc<Meter>, pool: PgPool) {
    let Meter::Redis(redis) = meter.as_ref() else {
        return;
    };
    loop {
        tokio::time::sleep(until_the_next_hour(SystemClock)).await;
        match redis.roll_up(&pool).await {
            Ok(tenants) => tracing::info!(tenants, "the finished hour was rolled up"),
            Err(error) => tracing::error!(%error, "the usage rollup failed"),
        }
    }
}

fn until_the_next_hour(clock: SystemClock) -> Duration {
    let elapsed = clock.now().get() % SECONDS_PER_HOUR;
    Duration::from_secs(SECONDS_PER_HOUR - elapsed)
}
