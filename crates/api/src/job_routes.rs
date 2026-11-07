use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use ferristrace_application::Stack;
use ferristrace_application::registry::ReadRepos;
use ferristrace_domain::{JobGrouping, JobSeries, JobSummary, TenantId};
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
    Job,
    Queue,
    System,
}

impl From<Grouped> for JobGrouping {
    fn from(grouped: Grouped) -> Self {
        match grouped {
            Grouped::Job => Self::Job,
            Grouped::Queue => Self::Queue,
            Grouped::System => Self::System,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Worked {
    jobs: Vec<Done>,
}

#[derive(Debug, Serialize)]
struct Done {
    job: String,
    queue: String,
    system: String,
    processed: u64,
    failed: u64,
    failure_rate: Option<f64>,
    per_second: f64,
    p50_us: u64,
    p95_us: u64,
    total_us: u64,
    last_seen_ms: u64,
}

fn done(summary: JobSummary, seconds: f64) -> Done {
    Done {
        #[allow(clippy::cast_precision_loss)]
        per_second: if seconds > 0.0 {
            summary.processed as f64 / seconds
        } else {
            0.0
        },
        failure_rate: summary.failure_rate(),
        job: summary.job.into_string(),
        queue: summary.queue.into_string(),
        system: summary.system.into_string(),
        processed: summary.processed,
        failed: summary.failed,
        p50_us: summary.p50_nanos / NANOS_PER_MICRO,
        p95_us: summary.p95_nanos / NANOS_PER_MICRO,
        total_us: summary.total_nanos / NANOS_PER_MICRO,
        last_seen_ms: summary.last_seen_unix_nanos / NANOS_PER_MILLI,
    }
}

type Beat = (u64, u64, u64, Option<u64>);

#[derive(Debug, Serialize)]
pub(crate) struct Working {
    step_ns: u64,
    processed: u64,
    failed: u64,
    failure_rate: Option<f64>,
    p50_ns: Option<u64>,
    p95_ns: Option<u64>,
    p99_ns: Option<u64>,
    points: Vec<Beat>,
}

impl From<JobSeries> for Working {
    fn from(series: JobSeries) -> Self {
        Self {
            step_ns: series.step_nanos,
            processed: series.processed,
            failed: series.failed,
            failure_rate: series.failure_rate(),
            p50_ns: series.p50_nanos,
            p95_ns: series.p95_nanos,
            p99_ns: series.p99_nanos,
            points: series
                .points
                .into_iter()
                .map(|point| {
                    (
                        point.start_unix_nanos,
                        point.processed,
                        point.failed,
                        point.p95_nanos,
                    )
                })
                .collect(),
        }
    }
}

pub(crate) async fn jobs<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(over): Query<Spanning>,
    Query(asking): Query<Asking>,
    headers: HeaderMap,
) -> Result<Json<Worked>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let seconds = over.seconds();
    let filter = over.filter()?;

    let found = state
        .app
        .jobs(&who, TenantId::new(org), asking.group.into(), &filter)
        .await?
        .into_iter()
        .map(|summary| done(summary, seconds))
        .collect();
    Ok(Json(Worked { jobs: found }))
}

pub(crate) async fn job_series<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(over): Query<Spanning>,
    headers: HeaderMap,
) -> Result<Json<Working>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let filter = over.filter()?;

    let series = state
        .app
        .job_series(&who, TenantId::new(org), &filter)
        .await?;
    Ok(Json(series.into()))
}
