use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::PostmortemRepos;
use ferristrace_domain::{
    BadPostmortem, IncidentId, Person, PostmortemBody, PostmortemError, PostmortemSummary,
    TenantId, Written,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, reading};
use crate::state::AppState;

pub fn postmortem_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: PostmortemRepos,
{
    Router::new()
        .route("/api/orgs/{org}/postmortems", get(listed::<S>))
        .route(
            "/api/orgs/{org}/incidents/{id}/postmortem",
            get(one::<S>).post(draft::<S>).patch(edit::<S>),
        )
        .route(
            "/api/orgs/{org}/incidents/{id}/postmortem/publish",
            post(publish::<S>),
        )
        .route(
            "/api/orgs/{org}/incidents/{id}/postmortem/unpublish",
            post(unpublish::<S>),
        )
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct AuthorBody {
    id: Uuid,
    email: Option<String>,
    name: Option<String>,
}

impl From<&Person> for AuthorBody {
    fn from(person: &Person) -> Self {
        Self {
            id: person.account.as_uuid(),
            email: person.email.as_ref().map(|email| email.as_str().to_owned()),
            name: person.name.as_deref().map(ToOwned::to_owned),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PostmortemBodyOut {
    id: Uuid,
    incident: Uuid,
    status: &'static str,
    body: String,
    author: Option<AuthorBody>,
    created_ms: u64,
    updated_ms: u64,
    published_ms: Option<u64>,
    edited: bool,
}

impl From<&Written> for PostmortemBodyOut {
    fn from(written: &Written) -> Self {
        let postmortem = &written.postmortem;
        Self {
            id: postmortem.id.as_uuid(),
            incident: postmortem.incident.as_uuid(),
            status: postmortem.status.as_str(),
            body: postmortem.body.as_str().to_owned(),
            author: written.author.as_ref().map(AuthorBody::from),
            created_ms: postmortem.created_ms,
            updated_ms: postmortem.updated_ms,
            published_ms: postmortem.published_ms,
            edited: postmortem.edited,
        }
    }
}

#[derive(Debug, Serialize)]
struct SummaryBody {
    incident: Uuid,
    status: &'static str,
}

impl From<&PostmortemSummary> for SummaryBody {
    fn from(summary: &PostmortemSummary) -> Self {
        Self {
            incident: summary.incident.as_uuid(),
            status: summary.status.as_str(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SummaryList {
    postmortems: Vec<SummaryBody>,
}

#[derive(Debug, Deserialize)]
pub struct Drafting {
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
pub struct EditAsked {
    body: String,
}

async fn listed<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<SummaryList>, PostmortemRefused>
where
    S::Repos: PostmortemRepos,
{
    let who = reading(&headers)?;
    let found = state
        .app
        .postmortem_summaries(&who, TenantId::new(org))
        .await?;
    Ok(Json(SummaryList {
        postmortems: found.iter().map(SummaryBody::from).collect(),
    }))
}

async fn one<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<PostmortemBodyOut>, PostmortemRefused>
where
    S::Repos: PostmortemRepos,
{
    let who = reading(&headers)?;
    let written = state
        .app
        .postmortem(&who, TenantId::new(org), IncidentId::new(id))
        .await?;
    Ok(Json(PostmortemBodyOut::from(&written)))
}

async fn draft<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    Query(drafting): Query<Drafting>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<PostmortemBodyOut>), PostmortemRefused>
where
    S::Repos: PostmortemRepos,
{
    let who = reading(&headers)?;
    let written = state
        .app
        .draft_postmortem(
            &who,
            TenantId::new(org),
            IncidentId::new(id),
            drafting.force,
        )
        .await?;
    tracing::info!(org = %org, incident = %id, "a postmortem was drafted");
    Ok((StatusCode::CREATED, Json(PostmortemBodyOut::from(&written))))
}

async fn edit<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<EditAsked>,
) -> Result<Json<PostmortemBodyOut>, PostmortemRefused>
where
    S::Repos: PostmortemRepos,
{
    let who = reading(&headers)?;
    let body = PostmortemBody::try_from(asked.body.as_str())?;
    let written = state
        .app
        .edit_postmortem(&who, TenantId::new(org), IncidentId::new(id), &body)
        .await?;
    Ok(Json(PostmortemBodyOut::from(&written)))
}

async fn publish<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<PostmortemBodyOut>, PostmortemRefused>
where
    S::Repos: PostmortemRepos,
{
    let who = reading(&headers)?;
    let written = state
        .app
        .publish_postmortem(&who, TenantId::new(org), IncidentId::new(id))
        .await?;
    Ok(Json(PostmortemBodyOut::from(&written)))
}

async fn unpublish<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<PostmortemBodyOut>, PostmortemRefused>
where
    S::Repos: PostmortemRepos,
{
    let who = reading(&headers)?;
    let written = state
        .app
        .unpublish_postmortem(&who, TenantId::new(org), IncidentId::new(id))
        .await?;
    Ok(Json(PostmortemBodyOut::from(&written)))
}

#[derive(Debug, thiserror::Error)]
pub enum PostmortemRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Body(#[from] BadPostmortem),
    #[error(transparent)]
    Refused(PostmortemError),
}

impl From<PostmortemError> for PostmortemRefused {
    fn from(error: PostmortemError) -> Self {
        match error {
            PostmortemError::NoSession => Self::Reaching(NotCarried::NoSession),
            PostmortemError::Postmortem(bad) => Self::Body(bad),
            other => Self::Refused(other),
        }
    }
}

fn status_of(error: PostmortemError) -> StatusCode {
    match error {
        PostmortemError::NoSession => StatusCode::UNAUTHORIZED,
        PostmortemError::NotYours
        | PostmortemError::NoSuchIncident
        | PostmortemError::NoSuchPostmortem => StatusCode::NOT_FOUND,
        PostmortemError::ReadOnly | PostmortemError::NotAdmin => StatusCode::FORBIDDEN,
        PostmortemError::Edited | PostmortemError::Published | PostmortemError::Exists => {
            StatusCode::CONFLICT
        }
        PostmortemError::Postmortem(_) => StatusCode::UNPROCESSABLE_ENTITY,
        PostmortemError::StoreUnreachable => StatusCode::SERVICE_UNAVAILABLE,
    }
}

impl IntoResponse for PostmortemRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Body(_) => (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),
            Self::Refused(PostmortemError::NotYours) => {
                (StatusCode::NOT_FOUND, "no such organisation".to_owned())
            }
            Self::Refused(error) => (status_of(error), error.to_string()),
        };
        if status.is_server_error() {
            tracing::error!(%message, "the postmortems could not be served");
        }
        (status, message).into_response()
    }
}
