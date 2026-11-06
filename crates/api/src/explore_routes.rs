use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::ReadRepos;
use ferristrace_domain::{
    AttributeFilter, AttributeKey, BadQuery, ExploreError, FacetFilter, Facets, ListedSpan,
    Measure, ProjectId, Query as TraceQuery, SeenAttribute, SeenValue, SeriesFilter, SeriesResult,
    ServiceFilter, ServiceSeries, ServiceSummary, Span, SpanFilter, SpanKind, SpanOrder,
    SpanStatus, TenantId, TraceFilter, TraceId, TraceOrder, TraceSummary, Window,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, reading};
use crate::placement::{Misplaced, Placed, PlacementBody};
use crate::query_body::{ExprBody, FieldRefBody, FieldRefOut};
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
    #[serde(default)]
    cluster: Option<String>,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    workload: Option<String>,
    #[serde(default)]
    pod: Option<String>,
    #[serde(default)]
    node: Option<String>,
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
    status_code: Option<u16>,
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
            status_code: summary.status_code,
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
    S::Repos: ReadRepos,
{
    Router::new()
        .route("/api/orgs/{org}/services", get(services::<S>))
        .route(
            "/api/orgs/{org}/services/{service}/operations",
            get(operations::<S>),
        )
        .route(
            "/api/orgs/{org}/services/{service}/series",
            get(service_series::<S>),
        )
        .route("/api/orgs/{org}/series", get(org_series::<S>))
        .route("/api/orgs/{org}/traces", get(traces::<S>))
        .route("/api/orgs/{org}/traces/search", post(search::<S>))
        .route("/api/orgs/{org}/traces/facets", post(facets::<S>))
        .route("/api/orgs/{org}/spans/search", post(span_search::<S>))
        .route("/api/orgs/{org}/spans/series", post(span_series::<S>))
        .route("/api/orgs/{org}/attributes", get(attributes::<S>))
        .route(
            "/api/orgs/{org}/attributes/{key}/values",
            get(attribute_values::<S>),
        )
        .route("/api/orgs/{org}/traces/{trace}", get(one::<S>))
        .route("/api/orgs/{org}/placements", get(placements::<S>))
        .with_state(state)
}

async fn traces<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<Asked>,
    headers: HeaderMap,
) -> Result<Json<Found>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let filter = filtered(asked, TraceQuery::anything())?;

    let found = state
        .app
        .traces(&who, TenantId::new(org), &filter)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(Found { traces: found }))
}

#[derive(Debug, Deserialize)]
pub struct Searched {
    #[serde(flatten)]
    asked: Asked,
    #[serde(default)]
    query: Option<ExprBody>,
}

async fn search<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(searched): Json<Searched>,
) -> Result<Json<Found>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let query = match searched.query {
        Some(body) => body.query()?,
        None => TraceQuery::anything(),
    };
    let filter = filtered(searched.asked, query)?;

    let found = state
        .app
        .traces(&who, TenantId::new(org), &filter)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(Found { traces: found }))
}

#[derive(Debug, Deserialize)]
pub struct SpanSearch {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    query: Option<ExprBody>,
    #[serde(default)]
    order: SpanOrdering,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SpanOrdering {
    #[default]
    Started,
    Duration,
}

#[derive(Debug, Serialize)]
pub struct FoundSpans {
    spans: Vec<SpanListed>,
}

#[derive(Debug, Serialize)]
struct SpanListed {
    trace_id: String,
    span_id: String,
    service: String,
    name: String,
    kind: &'static str,
    status: &'static str,
    status_code: Option<u16>,
    started_at_ms: u64,
    duration_us: u64,
}

impl From<ListedSpan> for SpanListed {
    fn from(span: ListedSpan) -> Self {
        Self {
            trace_id: hex::encode(span.trace_id.as_bytes()),
            span_id: hex::encode(span.span_id.as_bytes()),
            service: span.service.into_string(),
            name: span.name.into_string(),
            kind: named_kind(span.kind),
            status: named_status(span.status),
            status_code: span.status_code,
            started_at_ms: span.started_unix_nanos / NANOS_PER_MILLI,
            duration_us: span.duration_nanos / NANOS_PER_MICRO,
        }
    }
}

async fn span_search<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<SpanSearch>,
) -> Result<Json<FoundSpans>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let window = Window::new(
        asked.from.saturating_mul(NANOS_PER_MILLI),
        asked.to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LookRefused::Window)?;
    let mut filter = SpanFilter::over(window);
    if let Some(body) = asked.query {
        filter.query = body.query()?;
    }
    filter.project = asked.project.map(ProjectId::new);
    filter.order = match asked.order {
        SpanOrdering::Started => SpanOrder::Started,
        SpanOrdering::Duration => SpanOrder::Duration,
    };
    if let Some(limit) = asked.limit {
        filter = filter.at_most(limit);
    }

    let found = state
        .app
        .spans(&who, TenantId::new(org), &filter)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(FoundSpans { spans: found }))
}

#[derive(Debug, Deserialize)]
pub struct SeriesAsked {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    query: Option<ExprBody>,
    #[serde(default)]
    measure: Measuring,
    #[serde(default)]
    group_by: Option<FieldRefBody>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Measuring {
    #[default]
    Count,
    Errors,
    ErrorRate,
    Average,
    P50,
    P95,
    P99,
}

#[derive(Debug, Serialize)]
pub struct Plotted {
    step_ms: u64,
    series: Vec<Plot>,
}

#[derive(Debug, Serialize)]
struct Plot {
    group: Option<String>,
    points: Vec<(u64, Option<f64>)>,
}

impl From<SeriesResult> for Plotted {
    fn from(result: SeriesResult) -> Self {
        let starts = result.starts;
        Self {
            step_ms: result.step_nanos / NANOS_PER_MILLI,
            series: result
                .lines
                .into_iter()
                .map(|line| Plot {
                    group: line.group.map(String::from),
                    points: starts
                        .iter()
                        .map(|start| start / NANOS_PER_MILLI)
                        .zip(line.points)
                        .collect(),
                })
                .collect(),
        }
    }
}

async fn span_series<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<SeriesAsked>,
) -> Result<Json<Plotted>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let window = Window::new(
        asked.from.saturating_mul(NANOS_PER_MILLI),
        asked.to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LookRefused::Window)?;
    let query = match asked.query {
        Some(body) => body.query()?,
        None => TraceQuery::anything(),
    };
    let measure = match asked.measure {
        Measuring::Count => Measure::Count,
        Measuring::Errors => Measure::Errors,
        Measuring::ErrorRate => Measure::ErrorRate,
        Measuring::Average => Measure::Average,
        Measuring::P50 => Measure::P50,
        Measuring::P95 => Measure::P95,
        Measuring::P99 => Measure::P99,
    };
    let group = asked.group_by.map(FieldRefBody::field).transpose()?;
    let mut filter = SeriesFilter::new(window, query, measure, group)?;
    filter.project = asked.project.map(ProjectId::new);

    let plotted = state
        .app
        .span_series(&who, TenantId::new(org), &filter)
        .await?;
    Ok(Json(plotted.into()))
}

#[derive(Debug, Deserialize)]
pub struct Faceting {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    query: Option<ExprBody>,
    #[serde(default)]
    facets: Vec<FieldRefBody>,
    #[serde(default)]
    duration: bool,
}

#[derive(Debug, Serialize)]
pub struct Counted {
    facets: Vec<CountedField>,
    duration: Option<Spread>,
}

#[derive(Debug, Serialize)]
struct CountedField {
    #[serde(flatten)]
    field: FieldRefOut,
    values: Vec<Valued>,
}

#[derive(Debug, Serialize)]
struct Spread {
    min_us: u64,
    max_us: u64,
}

impl From<Facets> for Counted {
    fn from(found: Facets) -> Self {
        Self {
            facets: found
                .fields
                .into_iter()
                .map(|facet| CountedField {
                    field: FieldRefOut::from(&facet.field),
                    values: facet.values.into_iter().map(Valued::from).collect(),
                })
                .collect(),
            duration: found.duration.map(|range| Spread {
                min_us: range.min_nanos / NANOS_PER_MICRO,
                max_us: range.max_nanos.div_ceil(NANOS_PER_MICRO),
            }),
        }
    }
}

async fn facets<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Faceting>,
) -> Result<Json<Counted>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let window = Window::new(
        asked.from.saturating_mul(NANOS_PER_MILLI),
        asked.to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LookRefused::Window)?;
    let query = match asked.query {
        Some(body) => body.query()?,
        None => TraceQuery::anything(),
    };
    let fields = asked
        .facets
        .into_iter()
        .map(FieldRefBody::field)
        .collect::<Result<Vec<_>, _>>()?;
    let mut filter = FacetFilter::new(window, query, fields, asked.duration)?;
    filter.project = asked.project.map(ProjectId::new);

    let found = state.app.facets(&who, TenantId::new(org), &filter).await?;
    Ok(Json(found.into()))
}

fn filtered(asked: Asked, query: TraceQuery) -> Result<TraceFilter, LookRefused> {
    let window = Window::new(
        asked.from.saturating_mul(NANOS_PER_MILLI),
        asked.to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LookRefused::Window)?;
    let mut filter = TraceFilter::over(window);
    filter.query = query;
    filter.placement = PlacementBody {
        cluster: asked.cluster,
        namespace: asked.namespace,
        workload: asked.workload,
        pod: asked.pod,
        node: asked.node,
    }
    .placement()?;
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
    Ok(filter)
}

async fn one<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, trace)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Json<Waterfall>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let id = hex::decode(&trace)
        .ok()
        .and_then(|bytes| TraceId::try_from(bytes.as_slice()).ok())
        .ok_or(LookRefused::TraceId)?;

    let spans = state.app.trace(&who, TenantId::new(org), id).await?;
    if spans.is_empty() {
        return Err(LookRefused::NoSuchTrace);
    }
    Ok(Json(laid_out(id, spans)))
}

#[derive(Debug, Deserialize)]
pub struct Seen {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
}

async fn placements<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(seen): Query<Seen>,
    headers: HeaderMap,
) -> Result<Json<Placed>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let window = Window::new(
        seen.from.saturating_mul(NANOS_PER_MILLI),
        seen.to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LookRefused::Window)?;
    let found = state
        .app
        .placements(
            &who,
            TenantId::new(org),
            window,
            seen.project.map(ProjectId::new),
        )
        .await?;
    Ok(Json(found.into()))
}

#[derive(Debug, Deserialize)]
pub struct Catalogued {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    prefix: Option<String>,
}

impl Catalogued {
    fn filter(self) -> Result<AttributeFilter, LookRefused> {
        let window = Window::new(
            self.from.saturating_mul(NANOS_PER_MILLI),
            self.to.saturating_mul(NANOS_PER_MILLI),
        )
        .map_err(|_| LookRefused::Window)?;
        let mut filter = AttributeFilter::over(window);
        filter.project = self.project.map(ProjectId::new);
        filter.prefix = self
            .prefix
            .filter(|typed| !typed.trim().is_empty())
            .map(Into::into);
        Ok(filter)
    }
}

#[derive(Debug, Serialize)]
pub struct Keys {
    attributes: Vec<Attribute>,
}

#[derive(Debug, Serialize)]
struct Attribute {
    key: String,
    spans: u64,
    numeric: bool,
}

impl From<SeenAttribute> for Attribute {
    fn from(seen: SeenAttribute) -> Self {
        Self {
            key: seen.key.into_string(),
            spans: seen.spans,
            numeric: seen.numeric,
        }
    }
}

async fn attributes<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(catalogued): Query<Catalogued>,
    headers: HeaderMap,
) -> Result<Json<Keys>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let seen = state
        .app
        .attributes(&who, TenantId::new(org), &catalogued.filter()?)
        .await?;
    Ok(Json(Keys {
        attributes: seen.into_iter().map(Into::into).collect(),
    }))
}

#[derive(Debug, Serialize)]
pub struct Values {
    values: Vec<Valued>,
}

#[derive(Debug, Serialize)]
struct Valued {
    value: String,
    spans: u64,
}

impl From<SeenValue> for Valued {
    fn from(seen: SeenValue) -> Self {
        Self {
            value: seen.value.into_string(),
            spans: seen.spans,
        }
    }
}

async fn attribute_values<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, key)): Path<(Uuid, String)>,
    Query(catalogued): Query<Catalogued>,
    headers: HeaderMap,
) -> Result<Json<Values>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let key = AttributeKey::new(&key)?;
    let seen = state
        .app
        .attribute_values(&who, TenantId::new(org), &key, &catalogued.filter()?)
        .await?;
    Ok(Json(Values {
        values: seen.into_iter().map(Into::into).collect(),
    }))
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
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error("a window must end after it starts")]
    Window,
    #[error(transparent)]
    Placement(#[from] Misplaced),
    #[error(transparent)]
    Query(#[from] BadQuery),
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
            ExploreError::OutOfScope => Self::OutOfScope,
            ExploreError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for LookRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::OutOfScope => StatusCode::FORBIDDEN,
            Self::NoSuchOrg | Self::NoSuchTrace => StatusCode::NOT_FOUND,
            Self::Window | Self::TraceId | Self::Placement(_) | Self::Query(_) => {
                StatusCode::BAD_REQUEST
            }
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
    error_rate: f64,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    last_seen_ms: u64,
}

#[allow(clippy::cast_precision_loss)]
fn ratio(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

fn serving(summary: ServiceSummary, seconds: f64) -> Serving {
    Serving {
        #[allow(clippy::cast_precision_loss)]
        per_second: if seconds > 0.0 {
            summary.requests as f64 / seconds
        } else {
            0.0
        },
        error_rate: ratio(summary.errors, summary.requests),
        name: summary.name.into_string(),
        requests: summary.requests,
        errors: summary.errors,
        p50_us: summary.p50_nanos / NANOS_PER_MICRO,
        p95_us: summary.p95_nanos / NANOS_PER_MICRO,
        p99_us: summary.p99_nanos / NANOS_PER_MICRO,
        last_seen_ms: summary.last_seen_unix_nanos / NANOS_PER_MILLI,
    }
}

#[derive(Debug, Serialize)]
pub struct Operations {
    operations: Vec<Operation>,
}

#[derive(Debug, Serialize)]
struct Operation {
    name: String,
    requests: u64,
    errors: u64,
    p50_ns: u64,
    p95_ns: u64,
    p99_ns: u64,
    last_seen_ns: u64,
}

impl From<ServiceSummary> for Operation {
    fn from(summary: ServiceSummary) -> Self {
        Self {
            name: summary.name.into_string(),
            requests: summary.requests,
            errors: summary.errors,
            p50_ns: summary.p50_nanos,
            p95_ns: summary.p95_nanos,
            p99_ns: summary.p99_nanos,
            last_seen_ns: summary.last_seen_unix_nanos,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Stepped {
    step_ns: u64,
    requests: u64,
    errors: u64,
    p95_ns: Option<u64>,
    points: Vec<(u64, u64, u64, Option<u64>)>,
}

impl From<ServiceSeries> for Stepped {
    fn from(series: ServiceSeries) -> Self {
        Self {
            step_ns: series.step_nanos,
            requests: series.requests,
            errors: series.errors,
            p95_ns: series.p95_nanos,
            points: series
                .points
                .into_iter()
                .map(|point| {
                    (
                        point.start_unix_nanos,
                        point.requests,
                        point.errors,
                        point.p95_nanos,
                    )
                })
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Spanning {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    cluster: Option<String>,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    workload: Option<String>,
    #[serde(default)]
    pod: Option<String>,
    #[serde(default)]
    node: Option<String>,
}

impl Spanning {
    fn filter(self) -> Result<ServiceFilter, LookRefused> {
        let placement = PlacementBody {
            cluster: self.cluster,
            namespace: self.namespace,
            workload: self.workload,
            pod: self.pod,
            node: self.node,
        }
        .placement()?;
        let window = Window::new(
            self.from.saturating_mul(NANOS_PER_MILLI),
            self.to.saturating_mul(NANOS_PER_MILLI),
        )
        .map_err(|_| LookRefused::Window)?;
        let mut filter = ServiceFilter::over(window);
        filter.placement = placement;
        filter.project = self.project.map(ProjectId::new);
        filter.environment = self
            .environment
            .map(|typed| typed.trim().to_owned())
            .filter(|typed| !typed.is_empty())
            .map(Into::into);
        Ok(filter)
    }

    #[allow(clippy::cast_precision_loss)]
    fn seconds(&self) -> f64 {
        (self.to.saturating_sub(self.from) as f64) / 1000.0
    }
}

async fn services<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(over): Query<Spanning>,
    headers: HeaderMap,
) -> Result<Json<Running>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let seconds = over.seconds();
    let filter = over.filter()?;

    let found = state
        .app
        .services(&who, TenantId::new(org), &filter)
        .await?
        .into_iter()
        .map(|summary| serving(summary, seconds))
        .collect();
    Ok(Json(Running { services: found }))
}

async fn operations<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, service)): Path<(Uuid, String)>,
    Query(over): Query<Spanning>,
    headers: HeaderMap,
) -> Result<Json<Operations>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let filter = over.filter()?;

    let found = state
        .app
        .operations(&who, TenantId::new(org), &service, &filter)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(Operations { operations: found }))
}

async fn service_series<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, service)): Path<(Uuid, String)>,
    Query(over): Query<Spanning>,
    headers: HeaderMap,
) -> Result<Json<Stepped>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let filter = over.filter()?;

    let series = state
        .app
        .service_series(&who, TenantId::new(org), Some(&service), &filter)
        .await?;
    Ok(Json(series.into()))
}

async fn org_series<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(over): Query<Spanning>,
    headers: HeaderMap,
) -> Result<Json<Stepped>, LookRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let filter = over.filter()?;

    let series = state
        .app
        .service_series(&who, TenantId::new(org), None, &filter)
        .await?;
    Ok(Json(series.into()))
}
