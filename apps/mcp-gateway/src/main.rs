use std::sync::Arc;

use anyhow::Context;
use ferristrace_mcp_gateway::clock::SystemClock;
use ferristrace_mcp_gateway::decision::Recorder;
use ferristrace_mcp_gateway::{Gateway, export, router};
use tracing_subscriber::EnvFilter;

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let config = ferristrace_mcp_gateway::config::load().map_err(|error| anyhow::anyhow!(error))?;
    config.validate().context("the gateway configuration")?;
    let recorder = match (&config.ingest_key, &config.ingest_endpoint) {
        (Some(key), Some(endpoint)) if config.exports_decisions() => {
            let http = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .context("the ingest HTTP client")?;
            Recorder::exporting(export::spawn(http, endpoint.clone(), key.clone()))
        }
        _ => Recorder::logging(),
    };
    let gateway = Arc::new(Gateway::new(&config, SystemClock, recorder)?);
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("binding {}", config.listen))?;
    tracing::info!(
        listen = %config.listen,
        public_url = %config.public_base(),
        upstream = %config.upstream,
        issuer = %config.issuer,
        exporting_decisions = config.exports_decisions(),
        "ferristrace-mcp-gateway started"
    );
    axum::serve(listener, router(gateway))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}
