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
use ferristrace_api::{
    AppState, health_router, logs_server, metrics_server, router, trace_server, web_router,
};
use ferristrace_application::{Adapters, Ferristrace, Ingestion};
use ferristrace_clickhouse::{
    ClickhouseIssues, ClickhouseLogReader, ClickhouseMetricReader, ClickhouseReader,
    SpanStoreSettings, open_span_store,
};
use ferristrace_domain::telemetry::{Allowance, StaticQuota};
use ferristrace_domain::{
    ANNOUNCE_TICK_SECS, AckKey, Clock, DEPLOY_WATCH_TICK_SECS, Drained, Followed, Heralded,
    LinkKey, PAGE_TICK_SECS, Paged, ProvisionPolicy, TenantId, Tick,
};
use ferristrace_infra::{
    Argon2Hasher, HttpWebhooks, JwksSettings, JwksVerifier, MailSettings, MemoryMeter, MemorySink,
    OidcProvider, OidcSettings, PhotoStoreSettings, S3Photos, SecretSeal, SmtpMailer,
    StaticDirectory, SystemClock, WebhookSettings, ack_key_from_hex, status_key_from_hex,
};
use ferristrace_postgres::{
    CONTROL_PLANE, INVITE_WINDOW, Postgres, PostgresDirectory, load_allowances,
};
use ferristrace_redis::RedisMeter;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tonic::transport::Server;
use tracing_subscriber::EnvFilter;

use crate::adapters::{Directory, Gallery, Meter, Production, Relay, Sink, Stored};
use crate::cli::{Cli, Command, health, run_command};
use crate::config::{AgentSetup, Config, default_retention_days};
use crate::routes::{Held, State, control_routes, cookie_of};

const SECONDS_PER_HOUR: u64 = 3600;
const FLUSH_FALLBACK: Duration = Duration::from_secs(5);
const MIN_FLUSH_WAIT: Duration = Duration::from_millis(100);
const EVALUATE_EVERY: Duration = Duration::from_secs(60);
const DELIVER_EVERY: Duration = Duration::from_secs(10);
const ESCALATE_EVERY: Duration = Duration::from_secs(PAGE_TICK_SECS);
const ANNOUNCE_EVERY: Duration = Duration::from_secs(ANNOUNCE_TICK_SECS);
const FOLLOW_DEPLOYS_EVERY: Duration = Duration::from_secs(DEPLOY_WATCH_TICK_SECS);

#[derive(Default)]
struct Optional {
    span_store: Option<ClickhouseReader>,
    issue_store: Option<ClickhouseIssues>,
    log_store: Option<ClickhouseLogReader>,
    metric_store: Option<ClickhouseMetricReader>,
    oidc: Option<OidcProvider>,
    agents: Option<JwksVerifier>,
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
    let meter = Arc::new(meter_of(&config).await?);
    if let (Some(pool), Meter::Redis(_)) = (pool.as_ref(), meter.as_ref()) {
        let usage = Arc::new(usage_of(pool, Arc::clone(&meter))?);
        let seeded = usage
            .rehydrate_usage()
            .await
            .context("rehydrating the usage counters")?;
        tracing::info!(tenants = seeded, "usage counters rehydrated");
        tokio::spawn(roll_up_hourly(usage));
    }
    let Opened {
        sink,
        reader,
        grouping,
        logs,
        metrics,
    } = spans_of(&config).await?;
    let held = Held {
        explore: reader.is_some(),
        issues: grouping.is_some(),
    };
    let sink = Arc::new(sink);
    tokio::spawn(flush_store(Arc::clone(&sink)));
    let gallery = gallery_of(&config)?;
    let state = match pool.as_ref() {
        Some(pool) => {
            let optional = Optional {
                span_store: reader,
                issue_store: grouping,
                log_store: logs,
                metric_store: metrics,
                oidc: oidc_of(&config).await?,
                agents: agents_of(&config)?,
            };
            Some(control_plane(
                &config,
                pool,
                gallery,
                Arc::clone(&meter),
                optional,
            )?)
        }
        None => None,
    };
    let identity = control_routes(&config, state.as_ref(), held);
    let (stop, stopped) = watch::channel(false);
    let mut watching = watch_alerts(state.as_ref(), held.explore, &stopped);
    watching.extend(watch_pages(state.as_ref(), &stopped));
    watching.extend(watch_status(state.as_ref(), &stopped));
    watching.extend(watch_deploys(state.as_ref(), held.issues, &stopped));
    let gateway = Arc::new(gateway_of(
        directory_of(pool.as_ref(), default_retention_days(config.spans.as_ref())),
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
    stop.send_replace(true);
    for loop_ in watching {
        if let Err(error) = loop_.await {
            tracing::error!(%error, "an alert loop did not stop cleanly");
        }
    }
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
        guarded_webhooks(),
        Gallery::Nowhere,
        Arc::new(Meter::Memory(MemoryMeter::default())),
        Optional::default(),
    );
    let app = Ferristrace::<Production>::new(repos, adapters);
    run_command(command, config, &app, INVITE_WINDOW).await
}

fn control_plane(
    config: &Config,
    pool: &PgPool,
    gallery: Gallery,
    meter: Arc<Meter>,
    mut optional: Optional,
) -> anyhow::Result<State> {
    let hasher = Argon2Hasher::new().context("preparing the password hasher")?;
    let seal = seal_of(config)?;
    let ack = ack_key_of(config)?;
    let links = status_key_of(config)?;
    let mailer = relay_of(config)?;
    let repos = Postgres::new(pool.clone(), SystemClock)
        .with_hasher(hasher.clone())
        .with_seal(seal)
        .with_invite_window(INVITE_WINDOW);
    let agents = optional.agents.take();
    let adapters = adapters_of(
        hasher,
        mailer,
        webhooks_of(config),
        gallery,
        meter,
        optional,
    );
    let app = Arc::new(
        Ferristrace::<Production>::new(repos, adapters)
            .with_ack_key(ack)
            .with_status_key(links)
            .with_agent_tokens(agents),
    );
    Ok(AppState::new(app, cookie_of(config))
        .with_agent_policy(
            config.agent_setup().off_reason(),
            config.reserved_client_id().map(str::to_owned),
        )
        .with_default_retention_days(default_retention_days(config.spans.as_ref())))
}

fn adapters_of(
    hasher: Argon2Hasher,
    mailer: Relay,
    webhooks: HttpWebhooks,
    photos: Gallery,
    meter: Arc<Meter>,
    optional: Optional,
) -> Adapters<Production> {
    Adapters {
        clock: SystemClock,
        hasher,
        mailer,
        webhooks,
        photos,
        meter,
        span_store: optional.span_store,
        issue_store: optional.issue_store,
        log_store: optional.log_store,
        metric_store: optional.metric_store,
        oidc: optional.oidc,
        provision: ProvisionPolicy::default(),
    }
}

const DIRECTORY_CONNECTIONS: u32 = 2;

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

async fn meter_of(config: &Config) -> anyhow::Result<Meter> {
    let Some(url) = config.redis_url.as_deref() else {
        return Ok(Meter::Memory(MemoryMeter::default()));
    };

    let meter = RedisMeter::connect(url, SystemClock)
        .await
        .context("connecting to the usage counter store")?;
    Ok(Meter::Redis(meter))
}

fn usage_of(pool: &PgPool, meter: Arc<Meter>) -> anyhow::Result<Ferristrace<Production>> {
    let hasher = Argon2Hasher::new().context("preparing the password hasher")?;
    let repos = Postgres::new(pool.clone(), SystemClock);
    let adapters = adapters_of(
        hasher,
        Relay::Unreachable,
        guarded_webhooks(),
        Gallery::Nowhere,
        meter,
        Optional::default(),
    );
    Ok(Ferristrace::new(repos, adapters))
}

fn directory_of(pool: Option<&PgPool>, default_retention_days: u64) -> Directory {
    let Some(pool) = pool else {
        tracing::warn!(
            "no control plane is configured, so no ingest key can authenticate; \
             issue one with `ferristrace issue-key` once database_url is set"
        );
        return Directory::Nothing(StaticDirectory::default());
    };
    let own = PgPoolOptions::new()
        .max_connections(DIRECTORY_CONNECTIONS)
        .connect_lazy_with(pool.connect_options().as_ref().clone());
    Directory::ControlPlane(
        PostgresDirectory::new(own, SystemClock)
            .with_default_retention_days(default_retention_days),
    )
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

struct Opened {
    sink: Sink,
    reader: Option<ClickhouseReader>,
    grouping: Option<ClickhouseIssues>,
    logs: Option<ClickhouseLogReader>,
    metrics: Option<ClickhouseMetricReader>,
}

async fn spans_of(config: &Config) -> anyhow::Result<Opened> {
    let Some(spans) = config.spans.clone() else {
        tracing::warn!(
            "no span store is configured, so accepted spans, logs and metrics are held in memory \
             and lost when this process ends, and nothing can be read back; \
             set spans.url to keep them"
        );
        return Ok(Opened {
            sink: Sink::Memory(MemorySink::default()),
            reader: None,
            grouping: None,
            logs: None,
            metrics: None,
        });
    };
    let settings = SpanStoreSettings::from(spans);
    let store = open_span_store(&settings, SystemClock)
        .await
        .context("opening the span store")?;
    tracing::info!(
        url = %settings.url,
        retention_days = settings.retention_days,
        "spans, logs and metrics are written to the span store"
    );
    Ok(Opened {
        sink: Sink::Store(Box::new(Stored {
            spans: store.sink,
            logs: store.logs,
            metrics: store.metrics,
        })),
        reader: Some(store.reader),
        grouping: Some(store.issues),
        logs: Some(store.log_reader),
        metrics: Some(store.metric_reader),
    })
}

async fn flush_store(sink: Arc<Sink>) {
    let Sink::Store(store) = sink.as_ref() else {
        return;
    };
    loop {
        let wait = [
            store.spans.time_left().await,
            store.logs.time_left().await,
            store.metrics.time_left().await,
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(FLUSH_FALLBACK)
        .max(MIN_FLUSH_WAIT);
        tokio::time::sleep(wait).await;
        match store.spans.flush().await {
            Ok(0) => {}
            Ok(rows) => tracing::debug!(rows, "a span batch was committed on its deadline"),
            Err(error) => tracing::error!(%error, "a span batch could not be committed"),
        }
        match store.logs.flush().await {
            Ok(0) => {}
            Ok(rows) => tracing::debug!(rows, "a log batch was committed on its deadline"),
            Err(error) => tracing::error!(%error, "a log batch could not be committed"),
        }
        match store.metrics.flush().await {
            Ok(0) => {}
            Ok(rows) => tracing::debug!(rows, "a metric batch was committed on its deadline"),
            Err(error) => tracing::error!(%error, "a metric batch could not be committed"),
        }
    }
}

async fn flush_once(sink: &Sink) {
    let Sink::Store(store) = sink else {
        return;
    };
    match store.spans.flush().await {
        Ok(0) => {}
        Ok(rows) => tracing::info!(rows, "the last span batch was committed before leaving"),
        Err(error) => tracing::error!(%error, "the last span batch was lost"),
    }
    match store.logs.flush().await {
        Ok(0) => {}
        Ok(rows) => tracing::info!(rows, "the last log batch was committed before leaving"),
        Err(error) => tracing::error!(%error, "the last log batch was lost"),
    }
    match store.metrics.flush().await {
        Ok(0) => {}
        Ok(rows) => tracing::info!(rows, "the last metric batch was committed before leaving"),
        Err(error) => tracing::error!(%error, "the last metric batch was lost"),
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

fn agents_of(config: &Config) -> anyhow::Result<Option<JwksVerifier>> {
    match config.agent_setup() {
        AgentSetup::NoIssuer => {
            tracing::info!(
                "no agent issuer is configured, so no AI agent can read: set agents.issuer or oidc.issuer"
            );
            Ok(None)
        }
        AgentSetup::NoAudience { issuer } => {
            tracing::warn!(
                %issuer,
                "agents stay off: set agents.audience (FERRISTRACE_AGENTS__AUDIENCE) and add an audience mapper to the agents' FerrisKey clients, or any token of the realm would pass"
            );
            Ok(None)
        }
        AgentSetup::On { issuer, audience } => {
            let verifier = JwksVerifier::new(JwksSettings {
                issuer: issuer.clone(),
                audience: Some(audience),
            })
            .context("preparing the agent token verifier")?;
            tracing::info!(%issuer, "agent tokens are verified against the issuer's keys");
            Ok(Some(verifier))
        }
    }
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
            "no secrets.at_rest_key is configured, so nobody can keep a second factor or a webhook \
             channel; mint one with `openssl rand -hex 32`"
        );
        return Ok(None);
    };
    let seal = SecretSeal::from_hex(&secrets.at_rest_key).context("reading secrets.at_rest_key")?;
    tracing::info!("second factors and webhook secrets are sealed at rest");
    Ok(Some(seal))
}

fn ack_key_of(config: &Config) -> anyhow::Result<Option<AckKey>> {
    let Some(secrets) = &config.secrets else {
        tracing::warn!(
            "no secrets.at_rest_key is configured, so page emails carry no acknowledgement link; \
             pages are acknowledged in the app"
        );
        return Ok(None);
    };
    let key = ack_key_from_hex(&secrets.at_rest_key).context("reading secrets.at_rest_key")?;
    Ok(Some(key))
}

fn status_key_of(config: &Config) -> anyhow::Result<Option<LinkKey>> {
    let Some(secrets) = &config.secrets else {
        tracing::warn!(
            "no secrets.at_rest_key is configured, so nobody can subscribe to a status page by \
             email; the pages themselves are served"
        );
        return Ok(None);
    };
    let key = status_key_from_hex(&secrets.at_rest_key).context("reading secrets.at_rest_key")?;
    Ok(Some(key))
}

fn relay_of(config: &Config) -> anyhow::Result<Relay> {
    let Some(mail) = &config.mail else {
        tracing::warn!(
            "no mail relay is configured, so nobody can ask for a reset link and no alert is mailed; \
             hand a reset link over with `ferristrace reset-password`"
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
    tracing::info!(relay = %mail.relay, "password resets and alerts can be mailed");
    Ok(Relay::Smtp(Box::new(mailer)))
}

fn guarded_webhooks() -> HttpWebhooks {
    HttpWebhooks::new(WebhookSettings::default())
}

fn webhooks_of(config: &Config) -> HttpWebhooks {
    let base_url = config
        .alerts
        .base_url
        .clone()
        .or_else(|| config.mail.as_ref().map(|mail| mail.base_url.clone()))
        .unwrap_or_default();
    if config.alerts.allow_private_webhooks {
        tracing::warn!(
            "alerts.allow_private_webhooks is set, so a webhook may reach private, loopback \
             and link-local addresses, cloud metadata included"
        );
    }
    HttpWebhooks::new(WebhookSettings {
        allow_private: config.alerts.allow_private_webhooks,
        base_url,
        ..WebhookSettings::default()
    })
}

fn watch_alerts(
    state: Option<&State>,
    held: bool,
    stopped: &watch::Receiver<bool>,
) -> Vec<JoinHandle<()>> {
    let (Some(state), true) = (state, held) else {
        tracing::warn!(
            "alert rules are not evaluated: that needs both a control plane to hold the rules \
             and a span store to read the signals from"
        );
        return Vec::new();
    };
    tracing::info!("alert rules are evaluated every minute and delivered every ten seconds");
    vec![
        tokio::spawn(evaluate_alerts(Arc::clone(&state.app), stopped.clone())),
        tokio::spawn(deliver_alerts(Arc::clone(&state.app), stopped.clone())),
    ]
}

fn watch_pages(state: Option<&State>, stopped: &watch::Receiver<bool>) -> Option<JoinHandle<()>> {
    let state = state?;
    tracing::info!("pages are escalated and mailed every thirty seconds");
    Some(tokio::spawn(escalate_pages(
        Arc::clone(&state.app),
        stopped.clone(),
    )))
}

fn watch_status(state: Option<&State>, stopped: &watch::Receiver<bool>) -> Option<JoinHandle<()>> {
    let state = state?;
    tracing::info!("status page subscribers are mailed every thirty seconds");
    Some(tokio::spawn(announce_status(
        Arc::clone(&state.app),
        stopped.clone(),
    )))
}

fn watch_deploys(
    state: Option<&State>,
    held: bool,
    stopped: &watch::Receiver<bool>,
) -> Option<JoinHandle<()>> {
    let (Some(state), true) = (state, held) else {
        tracing::warn!(
            "deploys are not added to incident timelines: that needs both a control plane \
             to hold the incidents and a span store to read the releases from"
        );
        return None;
    };
    tracing::info!("deploys are added to open incidents every minute");
    Some(tokio::spawn(follow_deploys(
        Arc::clone(&state.app),
        stopped.clone(),
    )))
}

async fn follow_deploys(app: Arc<Ferristrace<Production>>, mut stopped: watch::Receiver<bool>) {
    let mut ticks = ticking(FOLLOW_DEPLOYS_EVERY);
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            _ = ticks.tick() => {}
        }
        match app.follow_deploys().await {
            Ok(Followed::Skipped) => tracing::debug!("another replica is following deploys"),
            Ok(Followed::Worked { entries: 0, .. }) => {}
            Ok(Followed::Worked { deploys, entries }) => {
                tracing::info!(deploys, entries, "deploys were added to open incidents");
            }
            Err(error) => tracing::error!(%error, "the deploys could not be followed"),
        }
    }
}

async fn announce_status(app: Arc<Ferristrace<Production>>, mut stopped: watch::Receiver<bool>) {
    let mut ticks = ticking(ANNOUNCE_EVERY);
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            _ = ticks.tick() => {}
        }
        match app.announce_status().await {
            Ok(Heralded::Skipped) => tracing::debug!("another replica is mailing subscribers"),
            Ok(Heralded::Worked {
                sent: 0,
                retried: 0,
                abandoned: 0,
            }) => {}
            Ok(Heralded::Worked {
                sent,
                retried,
                abandoned,
            }) => tracing::info!(
                sent,
                retried,
                abandoned,
                "status page subscribers were mailed"
            ),
            Err(error) => {
                tracing::error!(%error, "the status page subscribers could not be mailed");
            }
        }
    }
}

async fn escalate_pages(app: Arc<Ferristrace<Production>>, mut stopped: watch::Receiver<bool>) {
    let mut ticks = ticking(ESCALATE_EVERY);
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            _ = ticks.tick() => {}
        }
        match app.escalate_pages().await {
            Ok(Paged::Skipped) => tracing::debug!("another replica is escalating pages"),
            Ok(Paged::Worked {
                escalated: 0,
                sent: 0,
                retried: 0,
                abandoned: 0,
            }) => {}
            Ok(Paged::Worked {
                escalated,
                sent,
                retried,
                abandoned,
            }) => tracing::info!(
                escalated,
                sent,
                retried,
                abandoned,
                "pages were escalated and mailed"
            ),
            Err(error) => tracing::error!(%error, "the pages could not be escalated"),
        }
    }
}

fn ticking(period: Duration) -> tokio::time::Interval {
    let mut ticks = tokio::time::interval(period);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticks
}

async fn evaluate_alerts(app: Arc<Ferristrace<Production>>, mut stopped: watch::Receiver<bool>) {
    let mut ticks = ticking(EVALUATE_EVERY);
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            _ = ticks.tick() => {}
        }
        match app.evaluate_alerts().await {
            Ok(Tick::Skipped) => tracing::debug!("another replica evaluated this minute"),
            Ok(Tick::Evaluated {
                rules,
                transitions,
                failed,
                pruned,
            }) => {
                tracing::debug!(rules, transitions, pruned, "the alert rules were evaluated");
                if failed > 0 {
                    tracing::warn!(failed, "some alert rules could not read their signal");
                }
            }
            Err(error) => tracing::error!(%error, "the alert rules could not be evaluated"),
        }
    }
}

async fn deliver_alerts(app: Arc<Ferristrace<Production>>, mut stopped: watch::Receiver<bool>) {
    let mut ticks = ticking(DELIVER_EVERY);
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            _ = ticks.tick() => {}
        }
        match app.deliver_alerts().await {
            Ok(Drained::Skipped) => tracing::debug!("another replica is delivering"),
            Ok(Drained::Delivered {
                sent: 0,
                retried: 0,
                abandoned: 0,
            }) => {}
            Ok(Drained::Delivered {
                sent,
                retried,
                abandoned,
            }) => tracing::info!(
                sent,
                retried,
                abandoned,
                "alert notifications were delivered"
            ),
            Err(error) => tracing::error!(%error, "the alert notifications could not be delivered"),
        }
    }
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
        .add_service(trace_server(Arc::clone(&gateway)))
        .add_service(logs_server(Arc::clone(&gateway)))
        .add_service(metrics_server(gateway))
        .serve_with_shutdown(address, shutdown())
        .await
        .context("serving OTLP over gRPC")
}

async fn shutdown() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "the shutdown signal handler failed");
    }
}

async fn roll_up_hourly(usage: Arc<Ferristrace<Production>>) {
    loop {
        tokio::time::sleep(until_the_next_hour(SystemClock)).await;
        match usage.roll_up_usage().await {
            Ok(tenants) => tracing::info!(tenants, "the finished hour was rolled up"),
            Err(error) => tracing::error!(%error, "the usage rollup failed"),
        }
    }
}

fn until_the_next_hour(clock: SystemClock) -> Duration {
    let elapsed = clock.now().get() % SECONDS_PER_HOUR;
    Duration::from_secs(SECONDS_PER_HOUR - elapsed)
}
