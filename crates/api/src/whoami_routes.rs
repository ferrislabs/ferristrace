use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderName};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::ProfileRepos;
use ferristrace_domain::{
    AccountId, AccountName, BadAccountName, BadPhoto, MAX_PHOTO_BYTES, Photo, PhotoError, Portrait,
    ProfileError, SessionToken, TenantId, Who,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::session_cookie;
use crate::state::AppState;

const REQUESTED_WITH: HeaderName = HeaderName::from_static("x-requested-with");

const FOR_EVER: &str = "private, max-age=31536000, immutable";

#[derive(Debug, Serialize)]
pub struct Standing {
    account: Uuid,
    email: String,
    name: Option<String>,
    photo: Option<String>,
    active_org: Option<Uuid>,
    memberships: Vec<Belonging>,
}

#[derive(Debug, Serialize)]
struct Belonging {
    org: Uuid,
    name: String,
    role: &'static str,
}

impl From<Who> for Standing {
    fn from(who: Who) -> Self {
        Self {
            account: who.account.as_uuid(),
            email: who.email.as_str().to_owned(),
            name: who.name.map(Into::into),
            photo: who
                .photo
                .as_ref()
                .map(|portrait| photo_url(who.account.as_uuid(), portrait)),
            active_org: who.active_org.map(TenantId::as_uuid),
            memberships: who
                .memberships
                .into_iter()
                .map(|membership| Belonging {
                    org: membership.org.as_uuid(),
                    name: membership.name.into_string(),
                    role: membership.role.as_str(),
                })
                .collect(),
        }
    }
}

pub fn photo_url(account: Uuid, portrait: &Portrait) -> String {
    format!("/auth/accounts/{account}/photo?v={}", portrait.token)
}

#[derive(Debug, Deserialize)]
pub struct Naming {
    name: String,
}

pub fn me_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: ProfileRepos,
{
    Router::new()
        .route("/auth/me", get(standing::<S>).patch(rename::<S>))
        .route(
            "/auth/me/photo",
            post(keep::<S>)
                .delete(forget::<S>)
                .layer(DefaultBodyLimit::max(MAX_PHOTO_BYTES)),
        )
        .route("/auth/accounts/{account}/photo", get(served::<S>))
        .with_state(state)
}

async fn standing<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Json<Standing>, StandingRefused>
where
    S::Repos: ProfileRepos,
{
    let token = carried(&headers)?;

    Ok(Json(state.app.profile(&token).await?.into()))
}

async fn rename<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(asked): Json<Naming>,
) -> Result<Json<Standing>, StandingRefused>
where
    S::Repos: ProfileRepos,
{
    let token = carried(&headers)?;
    let chosen = asked.name.trim();
    let name = if chosen.is_empty() {
        None
    } else {
        Some(AccountName::try_from(chosen)?)
    };

    let who = state.app.rename_profile(&token, name.as_ref()).await?;
    tracing::info!(account = %who.account, named = who.name.is_some(), "an account was renamed");
    Ok(Json(who.into()))
}

async fn keep<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Standing>, StandingRefused>
where
    S::Repos: ProfileRepos,
{
    let token = carried(&headers)?;
    let photo = Photo::examined(body.to_vec())?;
    let kind = photo.kind();

    let who = state.app.put_profile_photo(&token, &photo).await?;
    tracing::info!(account = %who.account, %kind, "an account kept a photo");
    Ok(Json(who.into()))
}

async fn forget<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Json<Standing>, StandingRefused>
where
    S::Repos: ProfileRepos,
{
    let token = carried(&headers)?;

    let who = state.app.remove_profile_photo(&token).await?;
    tracing::info!(account = %who.account, "an account dropped its photo");
    Ok(Json(who.into()))
}

async fn served<S: Stack>(
    State(state): State<AppState<S>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, StandingRefused>
where
    S::Repos: ProfileRepos,
{
    let token = session_cookie::carried(&headers).ok_or(StandingRefused::NoSession)?;
    let (kind, bytes) = state
        .app
        .profile_photo(&token, AccountId::new(account))
        .await?;

    Ok((
        [(CONTENT_TYPE, kind.as_str()), (CACHE_CONTROL, FOR_EVER)],
        bytes,
    )
        .into_response())
}

fn carried(headers: &HeaderMap) -> Result<SessionToken, StandingRefused> {
    if !headers.contains_key(&REQUESTED_WITH) {
        return Err(StandingRefused::NotFromOurOrigin);
    }
    session_cookie::carried(headers).ok_or(StandingRefused::NoSession)
}

#[derive(Debug, thiserror::Error)]
pub enum StandingRefused {
    #[error("the request carries no x-requested-with header")]
    NotFromOurOrigin,
    #[error("the request carries no live session")]
    NoSession,
    #[error(transparent)]
    Name(#[from] BadAccountName),
    #[error(transparent)]
    Picture(#[from] BadPhoto),
    #[error("that account keeps no photo")]
    NoPhoto,
    #[error(transparent)]
    NoStore(PhotoError),
    #[error("the standing could not be read")]
    Unavailable,
}

impl From<ProfileError> for StandingRefused {
    fn from(error: ProfileError) -> Self {
        match error {
            ProfileError::NoSession => Self::NoSession,
            ProfileError::NoPhoto => Self::NoPhoto,
            ProfileError::Photos(refusal) => Self::NoStore(refusal),
            ProfileError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for StandingRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NotFromOurOrigin => StatusCode::FORBIDDEN,
            Self::NoSession => StatusCode::UNAUTHORIZED,
            Self::Name(_) | Self::Picture(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NoPhoto => StatusCode::NOT_FOUND,
            Self::NoStore(_) | Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a standing could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
