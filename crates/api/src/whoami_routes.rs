use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderName};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use ferristrace_domain::{
    AccountId, AccountName, Accounts, BadAccountName, BadPhoto, MAX_PHOTO_BYTES, Memberships,
    Photo, PhotoError, Photos, Portrait, Profile, ProfileError, SessionStore, SessionToken,
    TenantId, Who,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::session_cookie;

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

pub fn me_router<S, G, A, P>(profile: Arc<Profile<S, G, A, P>>) -> Router
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    A: Accounts + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/auth/me",
            get(standing::<S, G, A, P>).patch(rename::<S, G, A, P>),
        )
        .route(
            "/auth/me/photo",
            post(keep::<S, G, A, P>)
                .delete(forget::<S, G, A, P>)
                .layer(DefaultBodyLimit::max(MAX_PHOTO_BYTES)),
        )
        .route("/auth/accounts/{account}/photo", get(served::<S, G, A, P>))
        .with_state(profile)
}

async fn standing<S, G, A, P>(
    State(profile): State<Arc<Profile<S, G, A, P>>>,
    headers: HeaderMap,
) -> Result<Json<Standing>, StandingRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    A: Accounts + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
{
    let token = carried(&headers)?;

    Ok(Json(profile.read(&token).await?.into()))
}

async fn rename<S, G, A, P>(
    State(profile): State<Arc<Profile<S, G, A, P>>>,
    headers: HeaderMap,
    Json(asked): Json<Naming>,
) -> Result<Json<Standing>, StandingRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    A: Accounts + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    let chosen = asked.name.trim();
    let name = if chosen.is_empty() {
        None
    } else {
        Some(AccountName::try_from(chosen)?)
    };

    let who = profile.rename(&token, name.as_ref()).await?;
    tracing::info!(account = %who.account, named = who.name.is_some(), "an account was renamed");
    Ok(Json(who.into()))
}

async fn keep<S, G, A, P>(
    State(profile): State<Arc<Profile<S, G, A, P>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Standing>, StandingRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    A: Accounts + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    let photo = Photo::examined(body.to_vec())?;
    let kind = photo.kind();

    let who = profile.put_photo(&token, &photo).await?;
    tracing::info!(account = %who.account, %kind, "an account kept a photo");
    Ok(Json(who.into()))
}

async fn forget<S, G, A, P>(
    State(profile): State<Arc<Profile<S, G, A, P>>>,
    headers: HeaderMap,
) -> Result<Json<Standing>, StandingRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    A: Accounts + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
{
    let token = carried(&headers)?;

    let who = profile.remove_photo(&token).await?;
    tracing::info!(account = %who.account, "an account dropped its photo");
    Ok(Json(who.into()))
}

async fn served<S, G, A, P>(
    State(profile): State<Arc<Profile<S, G, A, P>>>,
    Path(account): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, StandingRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    A: Accounts + Send + Sync + 'static,
    P: Photos + Send + Sync + 'static,
{
    let token = session_cookie::carried(&headers).ok_or(StandingRefused::NoSession)?;
    let (kind, bytes) = profile.photo(&token, AccountId::new(account)).await?;

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
