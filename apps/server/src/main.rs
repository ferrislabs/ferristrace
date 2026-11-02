mod cli;
mod config;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use clap::Parser;
use ferristrace_api::{
    SessionCookie, SignInMethods, auth_router, catalogue_router, explore_router, health_router,
    issue_router, me_router, methods_router, oidc_router, org_router, password_router,
    reset_router, roster_router, router, second_factor_router, session_router, team_router,
    trace_server, usage_router, web_router,
};
use ferristrace_clickhouse::{
    ClickhouseIssues, ClickhouseReader, ClickhouseSink, SpanStoreSettings, open_span_store,
};
use ferristrace_domain::{
    AuthSources, Catalogue, ChangePassword, Clock, DecompressedBytes, Email, Explore,
    FederatedSignIn, Ingest, IngestKey, Issues, Mailer, Metering, NoPhotos, NoProvider,
    OrgSettings, Photo, PhotoError, Photos, Pictured, Portrait, Profile, ProvisionPolicy,
    ResetError, ResetPassword, ResetToken, Roster, Scope, SecondFactors, SecondStep,
    SessionLifetime, Sessions, SignIn, SinkError, Span, SpanSink, Squad, TenantDirectory, TenantId,
    UnknownKey, UsageMeter, UsageReader, Whoami,
};
use ferristrace_infra::{
    Allowance, Argon2Hasher, MailSettings, MemoryMeter, MemorySink, OidcProvider, OidcSettings,
    PhotoStoreSettings, S3Photos, SecretSeal, SmtpMailer, StaticDirectory, StaticQuota,
    SystemClock,
};
use ferristrace_postgres::{
    CONTROL_PLANE, LocalPasswordProvider, PostgresAccounts, PostgresDecisions, PostgresDirectory,
    PostgresFactors, PostgresFlows, PostgresMemberships, PostgresOrgs, PostgresProjects,
    PostgresResets, PostgresRoster, PostgresSessions, PostgresTeams, PostgresUsage,
    load_allowances,
};
use ferristrace_redis::RedisMeter;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tonic::transport::Server;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command, health, run_command};
use crate::config::Config;

const SECONDS_PER_HOUR: u64 = 3600;
const FLUSH_FALLBACK: Duration = Duration::from_secs(5);
const MIN_FLUSH_WAIT: Duration = Duration::from_millis(100);

type Gateway = Ingest<Directory, StaticQuota<Arc<Meter>>, Arc<Sink>, Arc<Meter>>;

#[derive(Debug)]
enum Sink {
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
enum Relay {
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
enum Gallery {
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
enum Meter {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let config = config::load().context("reading the configuration")?;
    match Cli::parse().command {
        Some(Command::Health) => return health(&config).await,
        Some(command) => return run_command(command, &config).await,
        None => {}
    }
    let pool = control_plane_pool(&config).await?;
    let allowances = allowances_of(&config, pool.as_ref()).await?;
    let meter = Arc::new(meter_of(&config, pool.as_ref()).await?);
    if let (Some(pool), Meter::Redis(_)) = (pool.as_ref(), meter.as_ref()) {
        tokio::spawn(roll_up_hourly(Arc::clone(&meter), pool.clone()));
    }
    let (sink, reader, grouping) = spans_of(&config).await?;
    let sink = Arc::new(sink);
    tokio::spawn(flush_spans(Arc::clone(&sink)));
    let gallery = Arc::new(gallery_of(&config)?);
    let identity = identity_routes(&config, pool.as_ref(), Arc::clone(&gallery))
        .await?
        .merge(explore_routes(pool.as_ref(), reader))
        .merge(issue_routes(pool.as_ref(), grouping))
        .merge(catalogue_routes(pool.as_ref()))
        .merge(org_routes(pool.as_ref(), gallery))
        .merge(roster_routes(pool.as_ref())?)
        .merge(team_routes(pool.as_ref()))
        .merge(usage_routes(pool.as_ref(), Arc::clone(&meter)));
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

#[derive(Debug)]
enum Directory {
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
) -> Gateway {
    let quota = StaticQuota::new(Arc::clone(&meter), allowances);
    Ingest::new(directory, quota, sink, meter)
}

type Looking = Explore<Arc<Store>, PostgresMemberships, ClickhouseReader>;
type Holding = Catalogue<Arc<Store>, PostgresMemberships, PostgresProjects>;
type Settling = OrgSettings<Arc<Store>, PostgresMemberships, PostgresOrgs, Arc<Gallery>>;
type Enrolling = Roster<Arc<Store>, PostgresMemberships, PostgresRoster<SystemClock>>;
type Arranging = Squad<Arc<Store>, PostgresMemberships, PostgresTeams>;
type Measuring = Metering<Arc<Store>, PostgresMemberships, Arc<Meter>, PostgresUsage>;
type Grouping =
    Issues<Arc<Store>, PostgresMemberships, ClickhouseIssues, PostgresDecisions, SystemClock>;

fn roster_routes(pool: Option<&PgPool>) -> anyhow::Result<Router> {
    let Some(pool) = pool else {
        return Ok(Router::new());
    };
    let sessions = Arc::new(PostgresSessions::new(
        pool.clone(),
        SystemClock,
        SessionLifetime::default(),
    ));
    let hasher = Argon2Hasher::new().context("preparing the password hasher")?;
    let roster: Arc<Enrolling> = Arc::new(Roster::new(
        Whoami::new(sessions, PostgresMemberships::new(pool.clone())),
        PostgresRoster::new(pool.clone(), hasher, SystemClock),
    ));
    Ok(roster_router(roster))
}

fn usage_routes(pool: Option<&PgPool>, meter: Arc<Meter>) -> Router {
    let Some(pool) = pool else {
        return Router::new();
    };
    let sessions = Arc::new(PostgresSessions::new(
        pool.clone(),
        SystemClock,
        SessionLifetime::default(),
    ));
    let metering: Arc<Measuring> = Arc::new(Metering::new(
        Whoami::new(sessions, PostgresMemberships::new(pool.clone())),
        meter,
        PostgresUsage::new(pool.clone()),
    ));
    usage_router(metering)
}

fn team_routes(pool: Option<&PgPool>) -> Router {
    let Some(pool) = pool else {
        return Router::new();
    };
    let sessions = Arc::new(PostgresSessions::new(
        pool.clone(),
        SystemClock,
        SessionLifetime::default(),
    ));
    let squad: Arc<Arranging> = Arc::new(Squad::new(
        Whoami::new(sessions, PostgresMemberships::new(pool.clone())),
        PostgresTeams::new(pool.clone()),
    ));
    team_router(squad)
}

fn org_routes(pool: Option<&PgPool>, gallery: Arc<Gallery>) -> Router {
    let Some(pool) = pool else {
        return Router::new();
    };
    let sessions = Arc::new(PostgresSessions::new(
        pool.clone(),
        SystemClock,
        SessionLifetime::default(),
    ));
    let settings: Arc<Settling> = Arc::new(OrgSettings::new(
        Whoami::new(sessions, PostgresMemberships::new(pool.clone())),
        PostgresOrgs::new(pool.clone()),
        gallery,
    ));
    org_router(settings)
}

fn catalogue_routes(pool: Option<&PgPool>) -> Router {
    let Some(pool) = pool else {
        return Router::new();
    };
    let sessions = Arc::new(PostgresSessions::new(
        pool.clone(),
        SystemClock,
        SessionLifetime::default(),
    ));
    let catalogue: Arc<Holding> = Arc::new(Catalogue::new(
        Whoami::new(sessions, PostgresMemberships::new(pool.clone())),
        PostgresProjects::new(pool.clone()),
    ));
    catalogue_router(catalogue)
}

fn issue_routes(pool: Option<&PgPool>, reader: Option<ClickhouseIssues>) -> Router {
    let (Some(pool), Some(reader)) = (pool, reader) else {
        tracing::warn!(
            "issues cannot be grouped: that needs both a control plane to authorise \
             the reader and a span store holding the exceptions to group"
        );
        return Router::new();
    };
    let sessions = Arc::new(PostgresSessions::new(
        pool.clone(),
        SystemClock,
        SessionLifetime::default(),
    ));
    let issues: Arc<Grouping> = Arc::new(Issues::new(
        Whoami::new(sessions, PostgresMemberships::new(pool.clone())),
        reader,
        PostgresDecisions::new(pool.clone()),
        SystemClock,
    ));
    issue_router(issues)
}

fn explore_routes(pool: Option<&PgPool>, reader: Option<ClickhouseReader>) -> Router {
    let (Some(pool), Some(reader)) = (pool, reader) else {
        tracing::warn!(
            "traces cannot be read back: that needs both a control plane to \
             authorise the reader and a span store to read from"
        );
        return Router::new();
    };
    let sessions = Arc::new(PostgresSessions::new(
        pool.clone(),
        SystemClock,
        SessionLifetime::default(),
    ));
    let explore: Arc<Looking> = Arc::new(Explore::new(
        Whoami::new(sessions, PostgresMemberships::new(pool.clone())),
        reader,
    ));
    explore_router(explore)
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

type LocalSource = LocalPasswordProvider<Argon2Hasher>;
type Store = PostgresSessions<SystemClock>;
type Factors = PostgresFactors<SystemClock>;
type PasswordSignIn = SignIn<LocalSource, NoProvider, PostgresAccounts, Arc<Store>, Factors>;
type Settings = SecondFactors<Factors, Arc<Store>, SystemClock>;
type Steps = SecondStep<Factors, Arc<Store>, SystemClock>;
type Resets = ResetPassword<PostgresResets<SystemClock>, Argon2Hasher, Relay>;
type Changes = ChangePassword<Arc<Store>, LocalSource, PostgresResets<SystemClock>, Argon2Hasher>;
type Me = Profile<Arc<Store>, PostgresMemberships, PostgresAccounts, Arc<Gallery>>;
type Rolling = Sessions<Arc<Store>, Store>;
type Federated =
    FederatedSignIn<OidcProvider, PostgresFlows<SystemClock>, PostgresAccounts, Arc<Store>>;

async fn identity_routes(
    config: &Config,
    pool: Option<&PgPool>,
    gallery: Arc<Gallery>,
) -> anyhow::Result<Router> {
    let offered = methods_router(SignInMethods {
        local_password: config.local_password && pool.is_some(),
        oidc: config.oidc.is_some() && pool.is_some(),
    });
    let Some(pool) = pool else {
        if config.local_password || config.oidc.is_some() {
            tracing::warn!("no control plane is configured, so nobody can sign in");
        }
        return Ok(offered);
    };

    let cookie = SessionCookie {
        secure: config.cookie_secure,
        ..SessionCookie::default()
    };
    let accounts = PostgresAccounts::new(pool.clone());
    let sessions = Arc::new(PostgresSessions::new(
        pool.clone(),
        SystemClock,
        SessionLifetime::default(),
    ));
    let me: Arc<Me> = Arc::new(Profile::new(
        Whoami::new(
            Arc::clone(&sessions),
            PostgresMemberships::new(pool.clone()),
        ),
        accounts.clone(),
        gallery,
    ));
    let rolling: Arc<Rolling> = Arc::new(Sessions::new(
        Arc::clone(&sessions),
        sessions.as_ref().clone(),
    ));
    let mut routes = offered.merge(me_router(me)).merge(session_router(rolling));

    if config.local_password {
        let hasher = Argon2Hasher::new().context("preparing the password hasher")?;
        let factors = PostgresFactors::new(pool.clone(), SystemClock, seal_of(config)?);
        let sign_in: Arc<PasswordSignIn> = Arc::new(SignIn::new(
            AuthSources::new(Some(LocalPasswordProvider::new(pool.clone(), hasher)), None),
            accounts.clone(),
            Arc::clone(&sessions),
            factors.clone(),
            ProvisionPolicy::default(),
        ));
        let settings: Arc<Settings> = Arc::new(SecondFactors::new(
            factors.clone(),
            Arc::clone(&sessions),
            SystemClock,
        ));
        let steps: Arc<Steps> =
            Arc::new(SecondStep::new(factors, Arc::clone(&sessions), SystemClock));
        routes = routes
            .merge(auth_router(sign_in, cookie))
            .merge(second_factor_router(settings, steps, cookie));
        tracing::info!("local passwords accepted");
    }

    if let Some(settings) = &config.oidc {
        let provider = OidcProvider::discover(OidcSettings {
            issuer: settings.issuer.clone(),
            client_id: settings.client_id.clone(),
            client_secret: settings.client_secret.clone(),
            redirect_uri: settings.redirect_uri.clone(),
        })
        .await
        .context("reaching the identity provider")?;
        let federated: Arc<Federated> = Arc::new(FederatedSignIn::new(
            provider,
            PostgresFlows::new(pool.clone(), SystemClock),
            accounts,
            Arc::clone(&sessions),
            ProvisionPolicy::default(),
        ));
        routes = routes.merge(oidc_router(federated, cookie, config.after_sign_in.clone()));
        tracing::info!(issuer = %settings.issuer, "federated sign in accepted");
    }

    if config.mail.is_some() && !config.local_password {
        tracing::warn!("a mail relay is configured but local passwords are off");
    }

    if config.local_password {
        let hasher = Argon2Hasher::new().context("preparing the password hasher")?;
        let reset: Arc<Resets> = Arc::new(ResetPassword::new(
            PostgresResets::new(pool.clone(), SystemClock),
            hasher.clone(),
            relay_of(config)?,
        ));
        let change: Arc<Changes> = Arc::new(ChangePassword::new(
            Arc::clone(&sessions),
            LocalPasswordProvider::new(pool.clone(), hasher.clone()),
            PostgresResets::new(pool.clone(), SystemClock),
            hasher,
        ));
        routes = routes
            .merge(reset_router(reset))
            .merge(password_router(change));
    }

    Ok(routes)
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
    gateway: Arc<Gateway>,
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

async fn serve_grpc(address: SocketAddr, gateway: Arc<Gateway>) -> anyhow::Result<()> {
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
