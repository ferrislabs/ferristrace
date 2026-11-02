use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::get;
use ferristrace_domain::{
    Consumption, Memberships, Metering, SessionStore, TenantId, UsageHistory, UsageReader, Window,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::carried;
use crate::explore_routes::LookRefused;

const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Deserialize)]
pub struct Asked {
    from: u64,
    to: u64,
}

#[derive(Debug, Serialize)]
pub struct Metered {
    ingested_bytes: u64,
    included_bytes: Option<u64>,
    hours: Vec<Hour>,
}

#[derive(Debug, Serialize)]
struct Hour {
    started_at_ms: u64,
    ingested_bytes: u64,
}

impl From<Consumption> for Metered {
    fn from(consumption: Consumption) -> Self {
        Self {
            ingested_bytes: consumption.ingested_bytes,
            included_bytes: consumption.included_bytes,
            hours: consumption
                .hours
                .into_iter()
                .map(|hour| Hour {
                    started_at_ms: hour.started_at_unix_nanos / NANOS_PER_MILLI,
                    ingested_bytes: hour.ingested_bytes,
                })
                .collect(),
        }
    }
}

pub fn usage_router<S, G, R, H>(metering: Arc<Metering<S, G, R, H>>) -> Router
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    R: UsageReader + Send + Sync + 'static,
    H: UsageHistory + Send + Sync + 'static,
{
    Router::new()
        .route("/api/orgs/{org}/usage", get(usage::<S, G, R, H>))
        .with_state(metering)
}

async fn usage<S, G, R, H>(
    State(metering): State<Arc<Metering<S, G, R, H>>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<Asked>,
    headers: HeaderMap,
) -> Result<Json<Metered>, LookRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    R: UsageReader + Send + Sync + 'static,
    H: UsageHistory + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    let window = Window::new(
        asked.from.saturating_mul(NANOS_PER_MILLI),
        asked.to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LookRefused::Window)?;
    let consumption = metering
        .consumption(&token, TenantId::new(org), window)
        .await?;
    Ok(Json(consumption.into()))
}
