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
    BadLogQuery, Buckets, ExploreError, LogBucket, LogCursor, LogPage, LogQuery, LogRecord,
    ProjectId, Severity, TenantId, TraceId, Window,
};
use serde::Serialize;
use uuid::Uuid;

use crate::carried::{NotCarried, reading};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

pub fn log_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: ReadRepos,
{
    Router::new()
        .route("/api/orgs/{org}/logs", get(search::<S>))
        .route("/api/orgs/{org}/logs/histogram", get(histogram::<S>))
        .with_state(state)
}

#[derive(Debug, Serialize)]
pub struct Records {
    records: Vec<Logged>,
    next: Option<String>,
}

#[derive(Debug, Serialize)]
struct Logged {
    time_ns: u64,
    observed_ns: u64,
    severity: &'static str,
    service: String,
    body: String,
    trace_id: Option<String>,
    span_id: Option<String>,
    attributes: BTreeMap<String, String>,
    resource: BTreeMap<String, String>,
}

impl From<LogRecord> for Logged {
    fn from(record: LogRecord) -> Self {
        Self {
            time_ns: record.time_unix_nanos,
            observed_ns: record.observed_unix_nanos,
            severity: record.severity.name(),
            service: record.service_name.into_string(),
            body: record.body.into_string(),
            trace_id: record.trace_id.map(|id| hex::encode(id.as_bytes())),
            span_id: record.span_id.map(|id| hex::encode(id.as_bytes())),
            attributes: paired(record.attributes),
            resource: paired(record.resource),
        }
    }
}

impl From<LogPage> for Records {
    fn from(page: LogPage) -> Self {
        Self {
            records: page.records.into_iter().map(Into::into).collect(),
            next: page.next.map(spelled),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Histogram {
    buckets: Vec<Counted>,
}

#[derive(Debug, Serialize)]
struct Counted {
    start_ns: u64,
    counts: Counts,
}

#[derive(Debug, Serialize)]
struct Counts {
    trace: u64,
    debug: u64,
    info: u64,
    warn: u64,
    error: u64,
    fatal: u64,
}

impl From<LogBucket> for Counted {
    fn from(bucket: LogBucket) -> Self {
        Self {
            start_ns: bucket.start_unix_nanos,
            counts: Counts {
                trace: bucket.count(Severity::Trace),
                debug: bucket.count(Severity::Debug),
                info: bucket.count(Severity::Info),
                warn: bucket.count(Severity::Warn),
                error: bucket.count(Severity::Error),
                fatal: bucket.count(Severity::Fatal),
            },
        }
    }
}

fn paired(attributes: BTreeMap<Box<str>, Box<str>>) -> BTreeMap<String, String> {
    attributes
        .into_iter()
        .map(|(key, value)| (key.into_string(), value.into_string()))
        .collect()
}

fn spelled(cursor: LogCursor) -> String {
    format!("{}.{}", cursor.time_unix_nanos, cursor.tiebreak)
}

fn cursor_of(spelling: &str) -> Result<LogCursor, LogRefused> {
    let (time, tiebreak) = spelling
        .split_once('.')
        .ok_or(LogRefused::Malformed("before"))?;
    Ok(LogCursor {
        time_unix_nanos: time.parse().map_err(|_| LogRefused::Malformed("before"))?,
        tiebreak: tiebreak
            .parse()
            .map_err(|_| LogRefused::Malformed("before"))?,
    })
}

#[derive(Debug, Default)]
struct Asked {
    from: Option<u64>,
    to: Option<u64>,
    services: Vec<Box<str>>,
    severities: Vec<Severity>,
    text: Option<Box<str>>,
    attributes: Vec<(Box<str>, Box<str>)>,
    trace: Option<TraceId>,
    project: Option<ProjectId>,
    limit: Option<String>,
    before: Option<String>,
    buckets: Option<u32>,
}

impl TryFrom<Vec<(String, String)>> for Asked {
    type Error = LogRefused;

    fn try_from(pairs: Vec<(String, String)>) -> Result<Self, LogRefused> {
        let mut asked = Self::default();
        for (key, value) in pairs {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.as_str() {
                "from" => asked.from = Some(number(value, "from")?),
                "to" => asked.to = Some(number(value, "to")?),
                "service" => asked.services.push(value.into()),
                "severity" => asked.severities.push(
                    Severity::try_from(value).map_err(|_| LogRefused::Malformed("severity"))?,
                ),
                "q" => asked.text = Some(value.into()),
                "attr" => {
                    let (name, held) = value
                        .split_once('=')
                        .filter(|(name, _)| !name.trim().is_empty())
                        .ok_or(LogRefused::Malformed("attr"))?;
                    asked.attributes.push((name.trim().into(), held.into()));
                }
                "trace" => asked.trace = Some(trace_of(value)?),
                "project" => {
                    asked.project = Some(ProjectId::new(
                        Uuid::parse_str(value).map_err(|_| LogRefused::Malformed("project"))?,
                    ));
                }
                "limit" => asked.limit = Some(value.to_owned()),
                "before" => asked.before = Some(value.to_owned()),
                "buckets" => asked.buckets = Some(number(value, "buckets")?),
                _ => {}
            }
        }
        Ok(asked)
    }
}

impl Asked {
    fn query(self, paged: bool) -> Result<LogQuery, LogRefused> {
        let (Some(from), Some(to)) = (self.from, self.to) else {
            return Err(LogRefused::Window);
        };
        let window = Window::new(
            from.saturating_mul(NANOS_PER_MILLI),
            to.saturating_mul(NANOS_PER_MILLI),
        )
        .map_err(|_| LogRefused::Window)?;
        let mut query = LogQuery::over(window);
        query.services = self.services;
        query.severities = self.severities;
        query.text = self.text;
        query.attributes = self.attributes;
        query.trace = self.trace;
        query.project = self.project;
        if paged {
            query.before = self.before.as_deref().map(cursor_of).transpose()?;
            if let Some(limit) = self.limit.as_deref() {
                query = query.at_most(number(limit, "limit")?)?;
            }
        }
        Ok(query)
    }
}

fn number<T: core::str::FromStr>(value: &str, name: &'static str) -> Result<T, LogRefused> {
    value.parse().map_err(|_| LogRefused::Malformed(name))
}

fn trace_of(value: &str) -> Result<TraceId, LogRefused> {
    hex::decode(value)
        .ok()
        .and_then(|bytes| TraceId::try_from(bytes.as_slice()).ok())
        .ok_or(LogRefused::Malformed("trace"))
}

async fn search<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(pairs): Query<Vec<(String, String)>>,
    headers: HeaderMap,
) -> Result<Json<Records>, LogRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&headers)?;
    let query = Asked::try_from(pairs)?.query(true)?;
    let page = state
        .app
        .search_logs(&who, TenantId::new(org), &query)
        .await?;
    Ok(Json(page.into()))
}

async fn histogram<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(pairs): Query<Vec<(String, String)>>,
    headers: HeaderMap,
) -> Result<Json<Histogram>, LogRefused>
where
    S::Repos: ReadRepos,
{
    let who = reading(&headers)?;
    let asked = Asked::try_from(pairs)?;
    let buckets = asked
        .buckets
        .map_or(Ok(Buckets::default()), Buckets::try_from)?;
    let query = asked.query(false)?;
    let counted = state
        .app
        .log_histogram(&who, TenantId::new(org), &query, buckets)
        .await?;
    Ok(Json(Histogram {
        buckets: counted.into_iter().map(Into::into).collect(),
    }))
}

impl From<NotCarried> for LogRefused {
    fn from(error: NotCarried) -> Self {
        Self::Reaching(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LogRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("a window must end after it starts")]
    Window,
    #[error("the {0} parameter is not well formed")]
    Malformed(&'static str),
    #[error(transparent)]
    OutOfRange(#[from] BadLogQuery),
    #[error("the logs could not be read")]
    Unavailable,
}

impl From<ExploreError> for LogRefused {
    fn from(error: ExploreError) -> Self {
        match error {
            ExploreError::NoSession => Self::Reaching(NotCarried::NoSession),
            ExploreError::NotYours => Self::NoSuchOrg,
            ExploreError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for LogRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::NoSuchOrg => StatusCode::NOT_FOUND,
            Self::Window | Self::Malformed(_) => StatusCode::BAD_REQUEST,
            Self::OutOfRange(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a log query could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
