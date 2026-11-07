use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use ferristrace_application::Stack;
use ferristrace_application::registry::ReadRepos;
use ferristrace_domain::{RequestGrouping, RequestSeries, RequestSummary, TenantId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::reading;
use crate::explore_routes::{LookRefused, NANOS_PER_MICRO, NANOS_PER_MILLI, Spanning};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub(crate) struct Asking {
    #[serde(default)]
    group: Grouped,
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Grouped {
    #[default]
    Route,
    Service,
    Method,
}

impl From<Grouped> for RequestGrouping {
    fn from(grouped: Grouped) -> Self {
        match grouped {
            Grouped::Route => Self::Route,
            Grouped::Service => Self::Service,
            Grouped::Method => Self::Method,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Served {
    routes: Vec<Taken>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Taken {
    route: String,
    service: String,
    method: String,
    requests: u64,
    answered: u64,
    refused: u64,
    broken: u64,
    refused_rate: Option<f64>,
    broken_rate: Option<f64>,
    per_second: f64,
    p50_us: u64,
    p95_us: u64,
    total_us: u64,
    last_seen_ms: u64,
}

fn taken(summary: RequestSummary, seconds: f64) -> Taken {
    Taken {
        #[allow(clippy::cast_precision_loss)]
        per_second: if seconds > 0.0 {
            summary.requests as f64 / seconds
        } else {
            0.0
        },
        refused_rate: summary.refused_rate(),
        broken_rate: summary.broken_rate(),
        route: summary.route.into_string(),
        service: summary.service.into_string(),
        method: summary.method.into_string(),
        requests: summary.requests,
        answered: summary.answered,
        refused: summary.refused,
        broken: summary.broken,
        p50_us: summary.p50_nanos / NANOS_PER_MICRO,
        p95_us: summary.p95_nanos / NANOS_PER_MICRO,
        total_us: summary.total_nanos / NANOS_PER_MICRO,
        last_seen_ms: summary.last_seen_unix_nanos / NANOS_PER_MILLI,
    }
}

type Beat = (u64, u64, u64, u64, Option<u64>);

#[derive(Debug, Default, Serialize)]
pub(crate) struct Flowing {
    step_ns: u64,
    requests: u64,
    answered: u64,
    refused: u64,
    broken: u64,
    refused_rate: Option<f64>,
    broken_rate: Option<f64>,
    p50_ns: Option<u64>,
    p95_ns: Option<u64>,
    p99_ns: Option<u64>,
    points: Vec<Beat>,
}

impl From<RequestSeries> for Flowing {
    fn from(series: RequestSeries) -> Self {
        Self {
            step_ns: series.step_nanos,
            requests: series.requests,
            answered: series.answered,
            refused: series.refused,
            broken: series.broken,
            refused_rate: series.refused_rate(),
            broken_rate: series.broken_rate(),
            p50_ns: series.p50_nanos,
            p95_ns: series.p95_nanos,
            p99_ns: series.p99_nanos,
            points: series
                .points
                .into_iter()
                .map(|point| {
                    (
                        point.start_unix_nanos,
                        point.requests,
                        point.refused,
                        point.broken,
                        point.p95_nanos,
                    )
                })
                .collect(),
        }
    }
}

pub(crate) async fn requests<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(over): Query<Spanning>,
    Query(asking): Query<Asking>,
    headers: HeaderMap,
) -> Result<Json<Served>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let seconds = over.seconds();
    let filter = over.filter()?;

    let found = state
        .app
        .requests(&who, TenantId::new(org), asking.group.into(), &filter)
        .await?
        .into_iter()
        .map(|summary| taken(summary, seconds))
        .collect();
    Ok(Json(Served { routes: found }))
}

pub(crate) async fn request_series<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(over): Query<Spanning>,
    headers: HeaderMap,
) -> Result<Json<Flowing>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let filter = over.filter()?;

    let series = state
        .app
        .request_series(&who, TenantId::new(org), &filter)
        .await?;
    Ok(Json(series.into()))
}
