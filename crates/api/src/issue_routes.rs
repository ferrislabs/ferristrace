use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::registry::IssueRepos;
use ferristrace_application::{IssueRuling, Stack};
use ferristrace_domain::{
    AccountId, Fingerprint, Issue, IssueError, IssueFilter, Occurrence, ProjectId, Status,
    TenantId, Window,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;
const DEFAULT_OCCURRENCES: u32 = 50;

#[derive(Debug, Deserialize)]
pub struct Asked {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct AskedOne {
    from: u64,
    to: u64,
    #[serde(default)]
    project: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct AskedSeen {
    from: u64,
    to: u64,
    project: Uuid,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct Decided {
    project: Uuid,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    #[allow(clippy::option_option)]
    owner: Option<Option<Uuid>>,
}

#[derive(Debug, Serialize)]
pub struct Found {
    issues: Vec<Listed>,
}

#[derive(Debug, Serialize)]
struct Listed {
    fingerprint: String,
    project: String,
    exception_type: String,
    message: String,
    service: String,
    events: u64,
    first_seen_at_ms: u64,
    last_seen_at_ms: u64,
    status: &'static str,
    owner: Option<String>,
    decided_at_ms: u64,
    regressed: bool,
}

impl From<Issue> for Listed {
    fn from(issue: Issue) -> Self {
        Self {
            fingerprint: issue.counted.print.to_string(),
            project: issue.counted.project.to_string(),
            exception_type: issue.counted.exception_type.into(),
            message: issue.counted.message.into(),
            service: issue.counted.service.into(),
            events: issue.counted.count,
            first_seen_at_ms: issue.counted.first_seen_unix_nanos / NANOS_PER_MILLI,
            last_seen_at_ms: issue.counted.last_seen_unix_nanos / NANOS_PER_MILLI,
            status: issue.decision.status.as_str(),
            owner: issue.decision.owner.map(|who| who.to_string()),
            decided_at_ms: issue.decision.decided_at_unix_nanos / NANOS_PER_MILLI,
            regressed: issue.regressed,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Seen {
    occurrences: Vec<Rendered>,
}

#[derive(Debug, Serialize)]
struct Rendered {
    trace_id: String,
    span_id: String,
    at_ms: u64,
    service: String,
    exception_type: String,
    message: String,
    stacktrace: Option<String>,
}

impl From<Occurrence> for Rendered {
    fn from(occurrence: Occurrence) -> Self {
        Self {
            trace_id: hex::encode(occurrence.trace_id.as_bytes()),
            span_id: hex::encode(occurrence.span_id.as_bytes()),
            at_ms: occurrence.time_unix_nanos / NANOS_PER_MILLI,
            service: occurrence.service.into(),
            exception_type: occurrence.exception_type.into(),
            message: occurrence.message.into(),
            stacktrace: occurrence.stacktrace.map(Into::into),
        }
    }
}

pub fn issue_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: IssueRepos,
{
    Router::new()
        .route("/api/orgs/{org}/issues", get(list::<S>))
        .route(
            "/api/orgs/{org}/issues/{print}",
            get(one::<S>).patch(decide::<S>),
        )
        .route(
            "/api/orgs/{org}/issues/{print}/occurrences",
            get(occurrences::<S>),
        )
        .with_state(state)
}

async fn list<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<Asked>,
    headers: HeaderMap,
) -> Result<Json<Found>, IssueRefused>
where
    S::Repos: IssueRepos,
{
    let token = carried(&headers)?;
    let mut filter = IssueFilter::over(a_window(asked.from, asked.to)?);
    filter.project = asked.project.map(ProjectId::new);
    filter.service = asked
        .service
        .filter(|typed| !typed.trim().is_empty())
        .map(Into::into);
    filter.text = asked
        .q
        .filter(|typed| !typed.trim().is_empty())
        .map(Into::into);
    if let Some(limit) = asked.limit {
        filter = filter.at_most(limit);
    }

    let found = state
        .app
        .list_issues(&token, TenantId::new(org), &filter)
        .await?
        .into_iter()
        .map(Listed::from)
        .collect();
    Ok(Json(Found { issues: found }))
}

async fn one<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, print)): Path<(Uuid, String)>,
    Query(asked): Query<AskedOne>,
    headers: HeaderMap,
) -> Result<Json<Listed>, IssueRefused>
where
    S::Repos: IssueRepos,
{
    let token = carried(&headers)?;
    let issue = state
        .app
        .issue(
            &token,
            TenantId::new(org),
            asked.project.map(ProjectId::new),
            a_print(&print)?,
            a_window(asked.from, asked.to)?,
        )
        .await?;
    Ok(Json(Listed::from(issue)))
}

async fn occurrences<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, print)): Path<(Uuid, String)>,
    Query(asked): Query<AskedSeen>,
    headers: HeaderMap,
) -> Result<Json<Seen>, IssueRefused>
where
    S::Repos: IssueRepos,
{
    let token = carried(&headers)?;
    let seen = state
        .app
        .issue_occurrences(
            &token,
            TenantId::new(org),
            ProjectId::new(asked.project),
            a_print(&print)?,
            a_window(asked.from, asked.to)?,
            asked.limit.unwrap_or(DEFAULT_OCCURRENCES),
        )
        .await?
        .into_iter()
        .map(Rendered::from)
        .collect();
    Ok(Json(Seen { occurrences: seen }))
}

async fn decide<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, print)): Path<(Uuid, String)>,
    headers: HeaderMap,
    Json(decided): Json<Decided>,
) -> Result<Response, IssueRefused>
where
    S::Repos: IssueRepos,
{
    let token = carried(&headers)?;
    let org = TenantId::new(org);
    let project = ProjectId::new(decided.project);
    let print = a_print(&print)?;
    let status = decided
        .status
        .as_deref()
        .map(|spelling| Status::parse(spelling).ok_or(IssueRefused::Status))
        .transpose()?;
    let ruling = IssueRuling {
        project,
        print,
        status,
        owner: decided.owner.map(|owner| owner.map(AccountId::new)),
    };
    state.app.rule_on_issue(&token, org, ruling).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn a_window(from: u64, to: u64) -> Result<Window, IssueRefused> {
    Window::new(
        from.saturating_mul(NANOS_PER_MILLI),
        to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| IssueRefused::Window)
}

fn a_print(spelling: &str) -> Result<Fingerprint, IssueRefused> {
    Fingerprint::try_from(spelling).map_err(|_| IssueRefused::Fingerprint)
}

#[derive(Debug, thiserror::Error)]
pub enum IssueRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error("a window must end after it starts")]
    Window,
    #[error("that is not a fingerprint")]
    Fingerprint,
    #[error("an issue is unresolved, resolved or ignored")]
    Status,
    #[error(transparent)]
    Refused(IssueError),
}

impl From<NotCarried> for IssueRefused {
    fn from(error: NotCarried) -> Self {
        Self::Reaching(error)
    }
}

impl From<IssueError> for IssueRefused {
    fn from(error: IssueError) -> Self {
        Self::Refused(error)
    }
}

impl IntoResponse for IssueRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Window => (StatusCode::BAD_REQUEST, Self::Window.to_string()),
            Self::Fingerprint => (StatusCode::BAD_REQUEST, Self::Fingerprint.to_string()),
            Self::Status => (StatusCode::UNPROCESSABLE_ENTITY, Self::Status.to_string()),
            Self::Refused(error @ IssueError::NoSession) => {
                (StatusCode::UNAUTHORIZED, error.to_string())
            }
            Self::Refused(error @ IssueError::NotYours) => {
                (StatusCode::FORBIDDEN, error.to_string())
            }
            Self::Refused(error @ IssueError::NoSuchIssue) => {
                (StatusCode::NOT_FOUND, error.to_string())
            }
            Self::Refused(error @ IssueError::Unavailable) => {
                tracing::error!(%error, "the issues could not be served");
                (StatusCode::SERVICE_UNAVAILABLE, error.to_string())
            }
        };
        (status, message).into_response()
    }
}
