use anyhow::Context;
use ferristrace_mcp::{ApiClient, router};
use tracing_subscriber::EnvFilter;

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let config = ferristrace_mcp::config::load().map_err(|error| anyhow::anyhow!(error))?;
    let api = ApiClient::new(&config.api_url).context("FERRISTRACE_MCP_API_URL")?;
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("binding {}", config.listen))?;
    tracing::info!(listen = %config.listen, api = %config.api_url, "ferristrace-mcp started");
    axum::serve(listener, router(api))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}
