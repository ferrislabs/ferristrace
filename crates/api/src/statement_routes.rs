use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use ferristrace_application::Stack;
use ferristrace_application::registry::ReadRepos;
use ferristrace_domain::{StatementGrouping, StatementSeries, StatementSummary, TenantId};
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
    Statement,
    Database,
    Operation,
}

impl From<Grouped> for StatementGrouping {
    fn from(grouped: Grouped) -> Self {
        match grouped {
            Grouped::Statement => Self::Statement,
            Grouped::Database => Self::Database,
            Grouped::Operation => Self::Operation,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Asked {
    statements: Vec<Said>,
}

#[derive(Debug, Serialize)]
struct Said {
    statement: String,
    database: String,
    system: String,
    calls: u64,
    errors: u64,
    error_rate: Option<f64>,
    per_second: f64,
    total_us: u64,
    p50_us: u64,
    p95_us: u64,
    last_seen_ms: u64,
}

fn said(summary: StatementSummary, seconds: f64) -> Said {
    Said {
        #[allow(clippy::cast_precision_loss)]
        per_second: if seconds > 0.0 {
            summary.calls as f64 / seconds
        } else {
            0.0
        },
        error_rate: summary.error_rate(),
        statement: summary.statement.into_string(),
        database: summary.database.into_string(),
        system: summary.system.into_string(),
        calls: summary.calls,
        errors: summary.errors,
        total_us: summary.total_nanos / NANOS_PER_MICRO,
        p50_us: summary.p50_nanos / NANOS_PER_MICRO,
        p95_us: summary.p95_nanos / NANOS_PER_MICRO,
        last_seen_ms: summary.last_seen_unix_nanos / NANOS_PER_MILLI,
    }
}

type Beat = (u64, u64, u64, Option<u64>);

#[derive(Debug, Serialize)]
pub(crate) struct Telling {
    step_ns: u64,
    calls: u64,
    errors: u64,
    error_rate: Option<f64>,
    p50_ns: Option<u64>,
    p95_ns: Option<u64>,
    p99_ns: Option<u64>,
    points: Vec<Beat>,
}

impl From<StatementSeries> for Telling {
    fn from(series: StatementSeries) -> Self {
        Self {
            step_ns: series.step_nanos,
            calls: series.calls,
            errors: series.errors,
            error_rate: series.error_rate(),
            p50_ns: series.p50_nanos,
            p95_ns: series.p95_nanos,
            p99_ns: series.p99_nanos,
            points: series
                .points
                .into_iter()
                .map(|point| {
                    (
                        point.start_unix_nanos,
                        point.calls,
                        point.errors,
                        point.p95_nanos,
                    )
                })
                .collect(),
        }
    }
}

pub(crate) async fn statements<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(over): Query<Spanning>,
    Query(asking): Query<Asking>,
    headers: HeaderMap,
) -> Result<Json<Asked>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let seconds = over.seconds();
    let filter = over.filter()?;

    let found = state
        .app
        .statements(&who, TenantId::new(org), asking.group.into(), &filter)
        .await?
        .into_iter()
        .map(|summary| said(summary, seconds))
        .collect();
    Ok(Json(Asked { statements: found }))
}

pub(crate) async fn statement_series<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(over): Query<Spanning>,
    headers: HeaderMap,
) -> Result<Json<Telling>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let filter = over.filter()?;

    let series = state
        .app
        .statement_series(&who, TenantId::new(org), &filter)
        .await?;
    Ok(Json(series.into()))
}
