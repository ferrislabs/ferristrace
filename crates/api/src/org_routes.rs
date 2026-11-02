use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use ferristrace_application::Stack;
use ferristrace_application::registry::OrgRepos;
use ferristrace_domain::{
    BadName, BadPhoto, BadRetention, MAX_PHOTO_BYTES, OrgName, Organisation, Photo, PhotoError,
    Portrait, RetentionDays, SettingError, TenantId,
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
    retention_days: Option<u32>,
    retention_cap_days: Option<u32>,
    default_retention_days: u64,
}

pub fn logo_url(org: Uuid, portrait: &Portrait) -> String {
    format!("/api/orgs/{org}/logo?v={}", portrait.token)
}

impl Card {
    fn of(org: Organisation, default_retention_days: u64) -> Self {
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
            retention_days: org.retention.map(RetentionDays::get),
            retention_cap_days: org.retention_cap.map(RetentionDays::get),
            default_retention_days,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Renaming {
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct Retaining {
    #[serde(deserialize_with = "Option::deserialize")]
    days: Option<u32>,
}

pub fn org_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: OrgRepos,
{
    Router::new()
        .route("/api/orgs/{org}", get(card::<S>).patch(rename::<S>))
        .route("/api/orgs/{org}/retention", patch(retain::<S>))
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
    Ok(Json(Card::of(card, state.default_retention_days)))
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
    Ok(Json(Card::of(card, state.default_retention_days)))
}

async fn retain<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Retaining>,
) -> Result<Json<Card>, OrgRefused>
where
    S::Repos: OrgRepos,
{
    let token = carried(&headers)?;
    let days = asked.days.map(RetentionDays::try_from).transpose()?;
    let card = state
        .app
        .set_org_retention(&token, TenantId::new(org), days)
        .await?;
    tracing::info!(org = %org, "an organisation changed its retention");
    Ok(Json(Card::of(card, state.default_retention_days)))
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
    Ok(Json(Card::of(card, state.default_retention_days)))
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
    Ok(Json(Card::of(card, state.default_retention_days)))
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
    #[error(transparent)]
    Retention(#[from] BadRetention),
    #[error("the plan keeps spans at most {0} days")]
    AboveCap(u32),
    #[error("retention is an owner's to set")]
    NotAnOwner,
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
            SettingError::OwnersOnly => Self::NotAnOwner,
            SettingError::AboveCap { cap } => Self::AboveCap(cap.get()),
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
            Self::Name(_) | Self::Picture(_) | Self::Retention(_) | Self::AboveCap(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::NoSuchOrg | Self::NoLogo => StatusCode::NOT_FOUND,
            Self::NotAnAdministrator | Self::NotAnOwner => StatusCode::FORBIDDEN,
            Self::NoStore(_) | Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "an organisation could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
