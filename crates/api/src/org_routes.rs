use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::OrgRepos;
use ferristrace_domain::{
    BadName, BadPhoto, MAX_PHOTO_BYTES, OrgName, Organisation, Photo, PhotoError, Portrait,
    SettingError, TenantId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::session_cookie;
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

const FOR_EVER: &str = "private, max-age=31536000, immutable";

#[derive(Debug, Serialize)]
pub struct Card {
    id: Uuid,
    name: String,
    plan: String,
    created_at_ms: u64,
    version: &'static str,
    logo: Option<String>,
}

pub fn logo_url(org: Uuid, portrait: &Portrait) -> String {
    format!("/api/orgs/{org}/logo?v={}", portrait.token)
}

impl From<Organisation> for Card {
    fn from(org: Organisation) -> Self {
        Self {
            id: org.id.as_uuid(),
            name: org.name.into_string(),
            plan: org.plan.into_string(),
            created_at_ms: org.created_at_unix_nanos / NANOS_PER_MILLI,
            version: env!("CARGO_PKG_VERSION"),
            logo: org
                .logo
                .as_ref()
                .map(|portrait| logo_url(org.id.as_uuid(), portrait)),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Renaming {
    name: String,
}

pub fn org_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: OrgRepos,
{
    Router::new()
        .route("/api/orgs/{org}", get(card::<S>).patch(rename::<S>))
        .route(
            "/api/orgs/{org}/logo",
            get(served::<S>)
                .post(keep::<S>)
                .delete(forget::<S>)
                .layer(DefaultBodyLimit::max(MAX_PHOTO_BYTES)),
        )
        .with_state(state)
}

async fn card<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Card>, OrgRefused>
where
    S::Repos: OrgRepos,
{
    let token = carried(&headers)?;
    let card = state.app.org_card(&token, TenantId::new(org)).await?;
    Ok(Json(card.into()))
}

async fn rename<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Renaming>,
) -> Result<Json<Card>, OrgRefused>
where
    S::Repos: OrgRepos,
{
    let token = carried(&headers)?;
    let name = OrgName::try_from(asked.name.as_str())?;
    let card = state
        .app
        .rename_org(&token, TenantId::new(org), &name)
        .await?;
    tracing::info!(org = %org, "an organisation was renamed");
    Ok(Json(card.into()))
}

async fn keep<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Card>, OrgRefused>
where
    S::Repos: OrgRepos,
{
    let token = carried(&headers)?;
    let logo = Photo::examined(body.to_vec())?;
    let kind = logo.kind();

    let card = state
        .app
        .put_org_logo(&token, TenantId::new(org), &logo)
        .await?;
    tracing::info!(org = %org, %kind, "an organisation kept a logo");
    Ok(Json(card.into()))
}

async fn forget<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Card>, OrgRefused>
where
    S::Repos: OrgRepos,
{
    let token = carried(&headers)?;

    let card = state
        .app
        .remove_org_logo(&token, TenantId::new(org))
        .await?;
    tracing::info!(org = %org, "an organisation dropped its logo");
    Ok(Json(card.into()))
}

async fn served<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, OrgRefused>
where
    S::Repos: OrgRepos,
{
    let token =
        session_cookie::carried(&headers).ok_or(OrgRefused::Reaching(NotCarried::NoSession))?;
    let (kind, bytes) = state.app.org_logo(&token, TenantId::new(org)).await?;

    Ok((
        [(CONTENT_TYPE, kind.as_str()), (CACHE_CONTROL, FOR_EVER)],
        bytes,
    )
        .into_response())
}

#[derive(Debug, thiserror::Error)]
pub enum OrgRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Name(#[from] BadName),
    #[error(transparent)]
    Picture(#[from] BadPhoto),
    #[error("that organisation keeps no logo")]
    NoLogo,
    #[error(transparent)]
    NoStore(PhotoError),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("changing an organisation is an administrator's to do")]
    NotAnAdministrator,
    #[error("the organisation could not be read")]
    Unavailable,
}

impl From<SettingError> for OrgRefused {
    fn from(error: SettingError) -> Self {
        match error {
            SettingError::NoSession => Self::Reaching(NotCarried::NoSession),
            SettingError::NotYours | SettingError::Gone => Self::NoSuchOrg,
            SettingError::NotYoursToChange => Self::NotAnAdministrator,
            SettingError::NoLogo => Self::NoLogo,
            SettingError::Photos(refusal) => Self::NoStore(refusal),
            SettingError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for OrgRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Name(_) | Self::Picture(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NoSuchOrg | Self::NoLogo => StatusCode::NOT_FOUND,
            Self::NotAnAdministrator => StatusCode::FORBIDDEN,
            Self::NoStore(_) | Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "an organisation could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
