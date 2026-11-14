use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::UsageRepos;
use ferristrace_domain::{Consumed, Consumption, TenantId, Window};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::carried;
use crate::explore_routes::LookRefused;
use crate::state::AppState;

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
    by_signal: BySignal,
    hours: Vec<Hour>,
}

#[derive(Debug, Serialize)]
struct BySignal {
    spans_bytes: u64,
    logs_bytes: u64,
    metrics_points: u64,
    unsplit_hours: usize,
}

#[derive(Debug, Serialize)]
struct Hour {
    started_at_ms: u64,
    ingested_bytes: u64,
    spans_bytes: Option<u64>,
    logs_bytes: Option<u64>,
    metrics_points: Option<u64>,
}

impl BySignal {
    fn over(hours: &[Consumed]) -> Self {
        let mut totals = Self {
            spans_bytes: 0,
            logs_bytes: 0,
            metrics_points: 0,
            unsplit_hours: 0,
        };
        for hour in hours {
            match hour.split {
                Some(split) => {
                    totals.spans_bytes = totals.spans_bytes.saturating_add(split.spans_bytes);
                    totals.logs_bytes = totals.logs_bytes.saturating_add(split.logs_bytes);
                    totals.metrics_points =
                        totals.metrics_points.saturating_add(split.metrics_points);
                }
                None => totals.unsplit_hours += 1,
            }
        }
        totals
    }
}

impl From<Consumption> for Metered {
    fn from(consumption: Consumption) -> Self {
        let by_signal = BySignal::over(&consumption.hours);
        Self {
            ingested_bytes: consumption.ingested_bytes,
            included_bytes: consumption.included_bytes,
            by_signal,
            hours: consumption
                .hours
                .into_iter()
                .map(|hour| Hour {
                    started_at_ms: hour.started_at_unix_nanos / NANOS_PER_MILLI,
                    ingested_bytes: hour.ingested_bytes,
                    spans_bytes: hour.split.map(|split| split.spans_bytes),
                    logs_bytes: hour.split.map(|split| split.logs_bytes),
                    metrics_points: hour.split.map(|split| split.metrics_points),
                })
                .collect(),
        }
    }
}

pub fn usage_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: UsageRepos,
{
    Router::new()
        .route("/api/orgs/{org}/usage", get(usage::<S>))
        .with_state(state)
}

async fn usage<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<Asked>,
    headers: HeaderMap,
) -> Result<Json<Metered>, LookRefused>
where
    S::Repos: UsageRepos,
{
    let token = carried(&state, &headers).await?;
    let window = Window::new(
        asked.from.saturating_mul(NANOS_PER_MILLI),
        asked.to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LookRefused::Window)?;
    let consumption = state
        .app
        .consumption(&token, TenantId::new(org), window)
        .await?;
    Ok(Json(consumption.into()))
}
