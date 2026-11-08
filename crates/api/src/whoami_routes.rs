use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::{InboxRepos, ProfileRepos};
use ferristrace_domain::{
    AccountId, AccountName, AccountTimezone, BadAccountName, BadPhoto, BadTimezone, Channels,
    InboxError, InboxItemKind, KINDS, MAX_PHOTO_BYTES, NotificationPreferences, Photo, PhotoError,
    Portrait, Preferences, ProfileError, RowDensity, StartPage, TenantId, TimestampPrecision, Who,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

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

#[derive(Debug, Serialize)]
pub struct PreferencesBody {
    timezone: String,
    precision: &'static str,
    density: &'static str,
    start_page: &'static str,
}

impl From<Preferences> for PreferencesBody {
    fn from(preferences: Preferences) -> Self {
        Self {
            timezone: preferences.timezone.as_str().to_owned(),
            precision: preferences.precision.as_str(),
            density: preferences.density.as_str(),
            start_page: preferences.start_page.as_str(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PreferencesAsked {
    timezone: String,
    precision: String,
    density: String,
    start_page: String,
}

impl PreferencesAsked {
    fn preferences(&self) -> Result<Preferences, StandingRefused> {
        Ok(Preferences {
            timezone: AccountTimezone::try_from(self.timezone.as_str())?,
            precision: TimestampPrecision::parse(&self.precision)
                .ok_or(StandingRefused::Malformed("precision"))?,
            density: RowDensity::parse(&self.density)
                .ok_or(StandingRefused::Malformed("density"))?,
            start_page: StartPage::parse(&self.start_page)
                .ok_or(StandingRefused::Malformed("start_page"))?,
        })
    }
}

pub fn me_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: ProfileRepos + InboxRepos,
{
    Router::new()
        .route("/auth/me", get(standing::<S>).patch(rename::<S>))
        .route(
            "/auth/me/preferences",
            get(preferences::<S>).put(set_preferences::<S>),
        )
        .route(
            "/auth/me/notification-preferences",
            get(notification_preferences::<S>).put(set_notification_preferences::<S>),
        )
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
    let token = carried(&state, &headers).await?;

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
    let token = carried(&state, &headers).await?;
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

async fn preferences<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Json<PreferencesBody>, StandingRefused>
where
    S::Repos: ProfileRepos,
{
    let token = carried(&state, &headers).await?;
    Ok(Json(state.app.preferences(&token).await?.into()))
}

async fn set_preferences<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(asked): Json<PreferencesAsked>,
) -> Result<Json<PreferencesBody>, StandingRefused>
where
    S::Repos: ProfileRepos,
{
    let token = carried(&state, &headers).await?;
    let preferences = asked.preferences()?;
    let saved = state.app.set_preferences(&token, &preferences).await?;
    Ok(Json(saved.into()))
}

#[derive(Debug, Serialize)]
pub struct ChannelBody {
    kind: &'static str,
    in_app: bool,
    email: bool,
}

#[derive(Debug, Serialize)]
pub struct NotificationPreferencesBody {
    email_enabled: bool,
    weekly_digest: bool,
    channels: Vec<ChannelBody>,
}

impl From<NotificationPreferences> for NotificationPreferencesBody {
    fn from(preferences: NotificationPreferences) -> Self {
        Self {
            email_enabled: preferences.email_enabled,
            weekly_digest: preferences.weekly_digest,
            channels: KINDS
                .into_iter()
                .map(|kind| {
                    let picked = preferences.channels_for(kind);
                    ChannelBody {
                        kind: kind.as_str(),
                        in_app: picked.in_app,
                        email: picked.email,
                    }
                })
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ChannelAsked {
    kind: String,
    in_app: bool,
    email: bool,
}

#[derive(Debug, Deserialize)]
pub struct NotificationPreferencesAsked {
    email_enabled: bool,
    weekly_digest: bool,
    channels: Vec<ChannelAsked>,
}

impl NotificationPreferencesAsked {
    fn preferences(&self) -> Result<NotificationPreferences, StandingRefused> {
        let mut channels = BTreeMap::new();
        for row in &self.channels {
            let kind = InboxItemKind::parse(&row.kind)
                .ok_or(StandingRefused::Malformed("channels.kind"))?;
            channels.insert(
                kind,
                Channels {
                    in_app: row.in_app,
                    email: row.email,
                },
            );
        }
        Ok(NotificationPreferences {
            email_enabled: self.email_enabled,
            weekly_digest: self.weekly_digest,
            channels,
        })
    }
}

async fn notification_preferences<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Json<NotificationPreferencesBody>, StandingRefused>
where
    S::Repos: InboxRepos,
{
    let token = carried(&state, &headers).await?;
    Ok(Json(
        state.app.notification_preferences(&token).await?.into(),
    ))
}

async fn set_notification_preferences<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(asked): Json<NotificationPreferencesAsked>,
) -> Result<Json<NotificationPreferencesBody>, StandingRefused>
where
    S::Repos: InboxRepos,
{
    let token = carried(&state, &headers).await?;
    let preferences = asked.preferences()?;
    let saved = state
        .app
        .set_notification_preferences(&token, &preferences)
        .await?;
    Ok(Json(saved.into()))
}

async fn keep<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Standing>, StandingRefused>
where
    S::Repos: ProfileRepos,
{
    let token = carried(&state, &headers).await?;
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
    let token = carried(&state, &headers).await?;

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
    let token = carried(&state, &headers).await?;
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

#[derive(Debug, thiserror::Error)]
pub enum StandingRefused {
    #[error("the request carries no live session")]
    NoSession,
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Name(#[from] BadAccountName),
    #[error(transparent)]
    Picture(#[from] BadPhoto),
    #[error(transparent)]
    Timezone(#[from] BadTimezone),
    #[error("the field {0} is not one this route understands")]
    Malformed(&'static str),
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

impl From<InboxError> for StandingRefused {
    fn from(error: InboxError) -> Self {
        match error {
            InboxError::NoSession => Self::NoSession,
            InboxError::NotYours | InboxError::NoSuchItem | InboxError::StoreUnreachable => {
                Self::Unavailable
            }
        }
    }
}

impl IntoResponse for StandingRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::NoSession => StatusCode::UNAUTHORIZED,
            Self::Reaching(ref reaching) => return reaching.into_response(),
            Self::Name(_) | Self::Picture(_) | Self::Timezone(_) | Self::Malformed(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::NoPhoto => StatusCode::NOT_FOUND,
            Self::NoStore(_) | Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a standing could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
