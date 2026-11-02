use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::ReadRepos;
use ferristrace_domain::{
    Aggregation, BadMetricQuery, Exemplar, ExploreError, MetricDescriptor, MetricError,
    MetricQuery, ProjectId, Series, SeriesSet, TenantId, Window,
};
use serde::Serialize;
use uuid::Uuid;

use crate::carried::{NotCarried, reading};
use crate::placement::{Misplaced, PlacementBody};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

pub fn metric_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: ReadRepos,
{
    Router::new()
        .route("/api/orgs/{org}/metrics", get(catalogue::<S>))
        .route("/api/orgs/{org}/metrics/{name}/series", get(series::<S>))
        .with_state(state)
}

#[derive(Debug, Serialize)]
pub struct Catalogue {
    metrics: Vec<Described>,
}

#[derive(Debug, Serialize)]
struct Described {
    name: String,
    kind: &'static str,
    unit: String,
    description: String,
    last_seen_ns: u64,
}

impl From<MetricDescriptor> for Described {
    fn from(metric: MetricDescriptor) -> Self {
        Self {
            name: metric.name.into_string(),
            kind: metric.kind.name(),
            unit: metric.unit.into_string(),
            description: metric.description.into_string(),
            last_seen_ns: metric.last_seen_unix_nanos,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Charted {
    step_ns: u64,
    series: Vec<Line>,
    exemplars: Vec<Sampled>,
}

#[derive(Debug, Serialize)]
struct Line {
    labels: BTreeMap<String, String>,
    points: Vec<(u64, Option<f64>)>,
}

#[derive(Debug, Serialize)]
struct Sampled {
    time_ns: u64,
    value: f64,
    trace_id: String,
    span_id: String,
}

impl From<Series> for Line {
    fn from(series: Series) -> Self {
        Self {
            labels: series
                .labels
                .into_iter()
                .map(|(key, value)| (key.into_string(), value.into_string()))
                .collect(),
            points: series.points,
        }
    }
}

impl Sampled {
    fn of(exemplar: &Exemplar) -> Option<Self> {
        Some(Self {
            time_ns: exemplar.time_unix_nanos,
            value: exemplar.value,
            trace_id: hex::encode(exemplar.trace_id?.as_bytes()),
            span_id: exemplar
                .span_id
                .map(|id| hex::encode(id.as_bytes()))
                .unwrap_or_default(),
        })
    }
}

impl From<SeriesSet> for Charted {
    fn from(set: SeriesSet) -> Self {
        Self {
            step_ns: set.step_nanos,
            series: set.series.into_iter().map(Into::into).collect(),
            exemplars: set.exemplars.iter().filter_map(Sampled::of).collect(),
        }
    }
}

#[derive(Debug, Default)]
struct Asked {
    placement: PlacementBody,
    from: Option<u64>,
    to: Option<u64>,
    text: Option<Box<str>>,
    group_by: Vec<Box<str>>,
    aggregation: Option<Aggregation>,
    project: Option<ProjectId>,
}

impl TryFrom<Vec<(String, String)>> for Asked {
    type Error = MetricRefused;

    fn try_from(pairs: Vec<(String, String)>) -> Result<Self, MetricRefused> {
        let mut asked = Self::default();
        for (key, value) in pairs {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.as_str() {
                "from" => asked.from = Some(number(value, "from")?),
                "to" => asked.to = Some(number(value, "to")?),
                "q" => asked.text = Some(value.into()),
                "group_by" => asked.group_by.push(value.into()),
                "agg" => {
                    asked.aggregation = Some(
                        Aggregation::try_from(value)
                            .map_err(|_| MetricRefused::Malformed("agg"))?,
                    );
                }
                "project" => {
                    asked.project = Some(ProjectId::new(
                        Uuid::parse_str(value).map_err(|_| MetricRefused::Malformed("project"))?,
                    ));
                }
                other => {
                    asked.placement.hold(other, value);
                }
            }
        }
        Ok(asked)
    }
}

impl Asked {
    fn query(self, name: &str) -> Result<MetricQuery, MetricRefused> {
        let (Some(from), Some(to)) = (self.from, self.to) else {
            return Err(MetricRefused::Window);
        };
        let window = Window::new(
            from.saturating_mul(NANOS_PER_MILLI),
            to.saturating_mul(NANOS_PER_MILLI),
        )
        .map_err(|_| MetricRefused::Window)?;
        let mut query = MetricQuery::over(window, name);
        query.placement = self.placement.placement()?;
        query.group_by = self.group_by;
        query.aggregation = self.aggregation;
        query.project = self.project;
        Ok(query)
    }
}

fn number<T: core::str::FromStr>(value: &str, name: &'static str) -> Result<T, MetricRefused> {
    value.parse().map_err(|_| MetricRefused::Malformed(name))
}

async fn catalogue<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(pairs): Query<Vec<(String, String)>>,
    headers: HeaderMap,
) -> Result<Json<Catalogue>, MetricRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let asked = Asked::try_from(pairs)?;
    let listed = state
        .app
        .metric_catalogue(
            &who,
            TenantId::new(org),
            asked.text.as_deref(),
            asked.project,
        )
        .await?;
    Ok(Json(Catalogue {
        metrics: listed.into_iter().map(Into::into).collect(),
    }))
}

async fn series<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, name)): Path<(Uuid, String)>,
    Query(pairs): Query<Vec<(String, String)>>,
    headers: HeaderMap,
) -> Result<Json<Charted>, MetricRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&state, &headers).await?;
    let query = Asked::try_from(pairs)?.query(&name)?;
    let charted = state
        .app
        .metric_series(&who, TenantId::new(org), &query)
        .await?;
    Ok(Json(charted.into()))
}

impl From<NotCarried> for MetricRefused {
    fn from(error: NotCarried) -> Self {
        Self::Reaching(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MetricRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("this agent holds no scope that allows this")]
    OutOfScope,
    #[error("no metric of that name was seen")]
    NoSuchMetric,
    #[error("a window must end after it starts")]
    Window,
    #[error("the {0} parameter is not well formed")]
    Malformed(&'static str),
    #[error(transparent)]
    Placement(#[from] Misplaced),
    #[error(transparent)]
    OutOfRange(#[from] BadMetricQuery),
    #[error("the metrics could not be read")]
    Unavailable,
}

impl From<ExploreError> for MetricRefused {
    fn from(error: ExploreError) -> Self {
        MetricError::from(error).into()
    }
}

impl From<MetricError> for MetricRefused {
    fn from(error: MetricError) -> Self {
        match error {
            MetricError::NoSession => Self::Reaching(NotCarried::NoSession),
            MetricError::NotYours => Self::NoSuchOrg,
            MetricError::OutOfScope => Self::OutOfScope,
            MetricError::StoreUnreachable => Self::Unavailable,
            MetricError::Unknown => Self::NoSuchMetric,
            MetricError::Query(bad) => Self::OutOfRange(bad),
        }
    }
}

impl IntoResponse for MetricRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::OutOfScope => StatusCode::FORBIDDEN,
            Self::NoSuchOrg | Self::NoSuchMetric => StatusCode::NOT_FOUND,
            Self::Window | Self::Malformed(_) | Self::Placement(_) => StatusCode::BAD_REQUEST,
            Self::OutOfRange(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a metric query could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
