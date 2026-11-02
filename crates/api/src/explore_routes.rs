use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::StandingRepos;
use ferristrace_domain::{
    ExploreError, ProjectId, ServiceSummary, Span, SpanKind, SpanStatus, TenantId, TraceFilter,
    TraceId, TraceOrder, TraceSummary, Window,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;
const NANOS_PER_MICRO: u64 = 1_000;

#[derive(Debug, Deserialize)]
pub struct Asked {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    errors: bool,
    #[serde(default)]
    at_least_us: Option<u64>,
    #[serde(default)]
    order: Order,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    #[default]
    Started,
    Duration,
    Spans,
}

impl From<Order> for TraceOrder {
    fn from(order: Order) -> Self {
        match order {
            Order::Started => Self::Started,
            Order::Duration => Self::Duration,
            Order::Spans => Self::Spans,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Found {
    traces: Vec<Listed>,
}

#[derive(Debug, Serialize)]
struct Listed {
    trace_id: String,
    root_service: String,
    root_name: String,
    started_at_ms: u64,
    duration_us: u64,
    spans: u64,
    errors: u64,
}

impl From<TraceSummary> for Listed {
    fn from(summary: TraceSummary) -> Self {
        Self {
            trace_id: hex::encode(summary.trace_id.as_bytes()),
            root_service: summary.root_service.into_string(),
            root_name: summary.root_name.into_string(),
            started_at_ms: summary.started_unix_nanos / NANOS_PER_MILLI,
            duration_us: summary.duration_nanos / NANOS_PER_MICRO,
            spans: summary.spans,
            errors: summary.errors,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Waterfall {
    trace_id: String,
    started_at_ms: u64,
    duration_us: u64,
    spans: Vec<Laid>,
}

#[derive(Debug, Serialize)]
struct Laid {
    span_id: String,
    parent_span_id: Option<String>,
    service: String,
    name: String,
    kind: &'static str,
    status: &'static str,
    start_offset_us: u64,
    duration_us: u64,
    attributes: BTreeMap<String, String>,
    events: Vec<Happened>,
}

#[derive(Debug, Serialize)]
struct Happened {
    name: String,
    offset_us: u64,
    attributes: BTreeMap<String, String>,
}

fn laid_out(trace: TraceId, spans: Vec<Span>) -> Waterfall {
    let earliest = spans
        .iter()
        .map(|span| span.start_unix_nanos)
        .min()
        .unwrap_or_default();
    let latest = spans
        .iter()
        .map(|span| span.end_unix_nanos)
        .max()
        .unwrap_or(earliest);
    Waterfall {
        trace_id: hex::encode(trace.as_bytes()),
        started_at_ms: earliest / NANOS_PER_MILLI,
        duration_us: latest.saturating_sub(earliest) / NANOS_PER_MICRO,
        spans: spans
            .into_iter()
            .map(|span| Laid {
                span_id: hex::encode(span.span_id.as_bytes()),
                parent_span_id: span.parent.map(|id| hex::encode(id.as_bytes())),
                service: span.service_name.into_string(),
                name: span.name.into_string(),
                kind: named_kind(span.kind),
                status: named_status(span.status),
                start_offset_us: span.start_unix_nanos.saturating_sub(earliest) / NANOS_PER_MICRO,
                duration_us: span.end_unix_nanos.saturating_sub(span.start_unix_nanos)
                    / NANOS_PER_MICRO,
                attributes: paired(span.attributes),
                events: span
                    .events
                    .into_iter()
                    .map(|event| Happened {
                        name: event.name.into_string(),
                        offset_us: event.time_unix_nanos.saturating_sub(earliest) / NANOS_PER_MICRO,
                        attributes: paired(event.attributes),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn paired(attributes: BTreeMap<Box<str>, Box<str>>) -> BTreeMap<String, String> {
    attributes
        .into_iter()
        .map(|(key, value)| (key.into_string(), value.into_string()))
        .collect()
}

fn named_kind(kind: SpanKind) -> &'static str {
    match kind {
        SpanKind::Unspecified => "unspecified",
        SpanKind::Internal => "internal",
        SpanKind::Server => "server",
        SpanKind::Client => "client",
        SpanKind::Producer => "producer",
        SpanKind::Consumer => "consumer",
    }
}

fn named_status(status: SpanStatus) -> &'static str {
    match status {
        SpanStatus::Unset => "unset",
        SpanStatus::Ok => "ok",
        SpanStatus::Error => "error",
    }
}

pub fn explore_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: StandingRepos,
{
    Router::new()
        .route("/api/orgs/{org}/services", get(services::<S>))
        .route("/api/orgs/{org}/traces", get(traces::<S>))
        .route("/api/orgs/{org}/traces/{trace}", get(one::<S>))
        .with_state(state)
}

async fn traces<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<Asked>,
    headers: HeaderMap,
) -> Result<Json<Found>, LookRefused>
where
    S::Repos: StandingRepos,
{
    let token = carried(&headers)?;
    let window = Window::new(
        asked.from.saturating_mul(NANOS_PER_MILLI),
        asked.to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LookRefused::Window)?;
    let mut filter = TraceFilter::over(window);
    filter.project = asked.project.map(ProjectId::new);
    filter.service = asked.service.map(Into::into);
    filter.name = asked
        .name
        .filter(|typed| !typed.trim().is_empty())
        .map(Into::into);
    filter.only_errors = asked.errors;
    filter.at_least_nanos = asked
        .at_least_us
        .map(|us| us.saturating_mul(NANOS_PER_MICRO));
    filter.order = asked.order.into();
    if let Some(limit) = asked.limit {
        filter = filter.at_most(limit);
    }

    let found = state
        .app
        .traces(&token, TenantId::new(org), &filter)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(Found { traces: found }))
}

async fn one<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, trace)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Json<Waterfall>, LookRefused>
where
    S::Repos: StandingRepos,
{
    let token = carried(&headers)?;
    let id = hex::decode(&trace)
        .ok()
        .and_then(|bytes| TraceId::try_from(bytes.as_slice()).ok())
        .ok_or(LookRefused::TraceId)?;

    let spans = state.app.trace(&token, TenantId::new(org), id).await?;
    if spans.is_empty() {
        return Err(LookRefused::NoSuchTrace);
    }
    Ok(Json(laid_out(id, spans)))
}

impl From<NotCarried> for LookRefused {
    fn from(error: NotCarried) -> Self {
        Self::Reaching(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LookRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("a window must end after it starts")]
    Window,
    #[error("that is not a trace identifier")]
    TraceId,
    #[error("no trace carries that identifier")]
    NoSuchTrace,
    #[error("the traces could not be read")]
    Unavailable,
}

impl From<ExploreError> for LookRefused {
    fn from(error: ExploreError) -> Self {
        match error {
            ExploreError::NoSession => Self::Reaching(NotCarried::NoSession),
            ExploreError::NotYours => Self::NoSuchOrg,
            ExploreError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for LookRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::NoSuchOrg | Self::NoSuchTrace => StatusCode::NOT_FOUND,
            Self::Window | Self::TraceId => StatusCode::BAD_REQUEST,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a trace lookup could not be served");
        }
        (status, self.to_string()).into_response()
    }
}

#[derive(Debug, Serialize)]
pub struct Running {
    services: Vec<Serving>,
}

#[derive(Debug, Serialize)]
struct Serving {
    name: String,
    requests: u64,
    errors: u64,
    per_second: f64,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    last_seen_ms: u64,
}

fn serving(summary: ServiceSummary, seconds: f64) -> Serving {
    Serving {
        name: summary.name.into_string(),
        requests: summary.requests,
        errors: summary.errors,
        #[allow(clippy::cast_precision_loss)]
        per_second: if seconds > 0.0 {
            summary.requests as f64 / seconds
        } else {
            0.0
        },
        p50_us: summary.p50_nanos / NANOS_PER_MICRO,
        p95_us: summary.p95_nanos / NANOS_PER_MICRO,
        p99_us: summary.p99_nanos / NANOS_PER_MICRO,
        last_seen_ms: summary.last_seen_unix_nanos / NANOS_PER_MILLI,
    }
}

async fn services<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(over): Query<Spanning>,
    headers: HeaderMap,
) -> Result<Json<Running>, LookRefused>
where
    S::Repos: StandingRepos,
{
    let token = carried(&headers)?;
    let window = Window::new(
        over.from.saturating_mul(NANOS_PER_MILLI),
        over.to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LookRefused::Window)?;
    #[allow(clippy::cast_precision_loss)]
    let seconds = (over.to.saturating_sub(over.from) as f64) / 1000.0;

    let found = state
        .app
        .services(
            &token,
            TenantId::new(org),
            window,
            over.project.map(ProjectId::new),
        )
        .await?
        .into_iter()
        .map(|summary| serving(summary, seconds))
        .collect();
    Ok(Json(Running { services: found }))
}

#[derive(Debug, Deserialize)]
pub struct Spanning {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
}
