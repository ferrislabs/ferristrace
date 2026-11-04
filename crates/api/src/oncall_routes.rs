use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::OncallRepos;
use ferristrace_domain::{
    AccountId, AckRefused, BadSchedule, Day, Handoff, OnCallNow, OnCallSchedule, OnCallStatus,
    OncallError, Override, OverrideId, Page, PageId, PageScope, PageSource, PageView, Person,
    Rostered, Rotation, RotationLength, ScheduleDraft, ScheduleId, ScheduleName, SchedulePatch,
    TeamId, TenantId, TimeOfDay, Timezone, Weekday,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::carried::{NotCarried, carried, reading};
use crate::state::AppState;

pub fn oncall_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: OncallRepos,
{
    Router::new()
        .route(
            "/api/orgs/{org}/teams/{team}/schedules",
            get(list::<S>).post(create::<S>),
        )
        .route(
            "/api/orgs/{org}/schedules/{id}",
            get(one::<S>).patch(update::<S>).delete(remove::<S>),
        )
        .route("/api/orgs/{org}/schedules/{id}/now", get(now::<S>))
        .route(
            "/api/orgs/{org}/schedules/{id}/overrides",
            post(add_override::<S>),
        )
        .route(
            "/api/orgs/{org}/schedules/{id}/overrides/{cover}",
            delete(remove_override::<S>),
        )
        .route("/api/orgs/{org}/oncall/now", get(whether::<S>))
        .route("/api/orgs/{org}/pages", get(pages::<S>))
        .route("/api/orgs/{org}/pages/{id}/ack", post(acknowledge::<S>))
        .route("/api/pages/ack", get(acknowledge_link::<S>))
        .with_state(state)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffBody {
    weekday: String,
    time: String,
}

impl HandoffBody {
    fn handoff(&self) -> Result<Handoff, OncallRefused> {
        Ok(Handoff {
            weekday: Weekday::parse(self.weekday.trim().to_ascii_lowercase().as_str())
                .ok_or(BadSchedule::Weekday)?,
            time: TimeOfDay::try_from(self.time.as_str())?,
        })
    }

    fn of(handoff: Handoff) -> Self {
        Self {
            weekday: handoff.weekday.as_str().to_owned(),
            time: handoff.time.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OverrideBody {
    id: Uuid,
    account: Uuid,
    from_ms: u64,
    to_ms: u64,
}

impl From<&Override> for OverrideBody {
    fn from(cover: &Override) -> Self {
        Self {
            id: cover.id.as_uuid(),
            account: cover.account.as_uuid(),
            from_ms: cover.from_ms,
            to_ms: cover.to_ms,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ScheduleBody {
    id: Uuid,
    team: Uuid,
    name: String,
    timezone: String,
    rotation: Vec<Uuid>,
    handoff: HandoffBody,
    length_days: u8,
    starts_on: String,
    overrides: Vec<OverrideBody>,
}

impl From<&Rostered> for ScheduleBody {
    fn from(rostered: &Rostered) -> Self {
        let schedule = &rostered.schedule;
        Self {
            id: schedule.id.as_uuid(),
            team: schedule.team.as_uuid(),
            name: schedule.name.as_str().to_owned(),
            timezone: schedule.timezone.name().to_owned(),
            rotation: schedule
                .rotation
                .people()
                .iter()
                .map(|person| person.as_uuid())
                .collect(),
            handoff: HandoffBody::of(schedule.handoff),
            length_days: schedule.length.days(),
            starts_on: schedule.starts_on.to_string(),
            overrides: rostered.overrides.iter().map(OverrideBody::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ScheduleList {
    schedules: Vec<ScheduleBody>,
}

#[derive(Debug, Deserialize)]
pub struct DraftBody {
    name: String,
    timezone: String,
    #[serde(default)]
    rotation: Vec<Uuid>,
    handoff: HandoffBody,
    length_days: u64,
    starts_on: String,
}

fn rotation_of(people: &[Uuid]) -> Result<Rotation, OncallRefused> {
    Ok(Rotation::new(
        people.iter().copied().map(AccountId::new).collect(),
    )?)
}

impl DraftBody {
    fn draft(&self) -> Result<ScheduleDraft, OncallRefused> {
        Ok(ScheduleDraft {
            name: ScheduleName::try_from(self.name.as_str())?,
            timezone: Timezone::try_from(self.timezone.as_str())?,
            rotation: rotation_of(&self.rotation)?,
            handoff: self.handoff.handoff()?,
            length: RotationLength::try_from(self.length_days)?,
            starts_on: Day::try_from(self.starts_on.as_str())?,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct PatchBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    rotation: Option<Vec<Uuid>>,
    #[serde(default)]
    handoff: Option<HandoffBody>,
    #[serde(default)]
    length_days: Option<u64>,
    #[serde(default)]
    starts_on: Option<String>,
}

impl PatchBody {
    fn patch(&self) -> Result<SchedulePatch, OncallRefused> {
        Ok(SchedulePatch {
            name: self
                .name
                .as_deref()
                .map(ScheduleName::try_from)
                .transpose()?,
            timezone: self
                .timezone
                .as_deref()
                .map(Timezone::try_from)
                .transpose()?,
            rotation: self.rotation.as_deref().map(rotation_of).transpose()?,
            handoff: self
                .handoff
                .as_ref()
                .map(HandoffBody::handoff)
                .transpose()?,
            length: self.length_days.map(RotationLength::try_from).transpose()?,
            starts_on: self.starts_on.as_deref().map(Day::try_from).transpose()?,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct OverrideAsked {
    account: Uuid,
    from_ms: u64,
    to_ms: u64,
}

#[derive(Debug, Serialize)]
struct PersonBody {
    id: Uuid,
    email: Option<String>,
    name: Option<String>,
}

impl From<&Person> for PersonBody {
    fn from(person: &Person) -> Self {
        Self {
            id: person.account.as_uuid(),
            email: person.email.as_ref().map(|email| email.as_str().to_owned()),
            name: person.name.as_deref().map(ToOwned::to_owned),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct NowBody {
    account: Option<PersonBody>,
    until_ms: Option<u64>,
}

impl From<&OnCallNow> for NowBody {
    fn from(now: &OnCallNow) -> Self {
        Self {
            account: now.person.as_ref().map(PersonBody::from),
            until_ms: now.until_ms,
        }
    }
}

fn source_of(page: &Page) -> Value {
    match page.source {
        PageSource::Alert { rule, event } => json!({
            "kind": "alert",
            "rule": rule.as_uuid(),
            "event": event.as_uuid(),
            "name": &*page.headline,
        }),
        PageSource::Incident(incident) => json!({
            "kind": "incident",
            "incident": incident.as_uuid(),
            "title": &*page.headline,
        }),
    }
}

#[derive(Debug, Serialize)]
pub struct PageBody {
    id: Uuid,
    schedule: Value,
    target: PersonBody,
    source: Value,
    headline: String,
    state: &'static str,
    step: u32,
    created_ms: u64,
    escalate_at_ms: Option<u64>,
    acknowledged_by: Option<Uuid>,
    acknowledged_ms: Option<u64>,
    resolved_ms: Option<u64>,
}

impl From<&PageView> for PageBody {
    fn from(view: &PageView) -> Self {
        let page = &view.page;
        Self {
            id: page.id.as_uuid(),
            schedule: json!({
                "id": page.schedule.as_uuid(),
                "name": view.schedule.as_ref().map(ScheduleName::as_str),
            }),
            target: PersonBody::from(&view.target),
            source: source_of(page),
            headline: page.headline.to_string(),
            state: page.state.as_str(),
            step: page.step,
            created_ms: page.created_ms,
            escalate_at_ms: page.escalate_at_ms,
            acknowledged_by: page.acknowledged_by.map(AccountId::as_uuid),
            acknowledged_ms: page.acknowledged_ms,
            resolved_ms: page.resolved_ms,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PageList {
    pages: Vec<PageBody>,
}

#[derive(Debug, Deserialize)]
pub struct PageListing {
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WhetherBody {
    on_call: bool,
    schedules: Vec<String>,
}

impl From<&OnCallStatus> for WhetherBody {
    fn from(status: &OnCallStatus) -> Self {
        Self {
            on_call: status.on_call,
            schedules: status
                .schedules
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct HolderBody {
    email: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DutyBody {
    id: Uuid,
    name: String,
    team: Uuid,
    on_call: Option<HolderBody>,
    until_ms: Option<u64>,
}

impl From<&OnCallSchedule> for DutyBody {
    fn from(duty: &OnCallSchedule) -> Self {
        Self {
            id: duty.schedule.as_uuid(),
            name: duty.name.as_str().to_owned(),
            team: duty.team.as_uuid(),
            on_call: duty.now.person.as_ref().map(|person| HolderBody {
                email: person.email.as_ref().map(|email| email.as_str().to_owned()),
                name: person.name.as_deref().map(ToOwned::to_owned),
            }),
            until_ms: duty.now.until_ms,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Everyone {
    schedules: Vec<DutyBody>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Whose {
    One(WhetherBody),
    Everyone(Everyone),
}

#[derive(Debug, Deserialize)]
pub struct Whether {
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AckLink {
    #[serde(default)]
    token: String,
}

async fn list<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, team)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<ScheduleList>, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let who = reading(&state, &headers).await?;
    let held = state
        .app
        .team_schedules(&who, TenantId::new(org), TeamId::new(team))
        .await?;
    Ok(Json(ScheduleList {
        schedules: held.iter().map(ScheduleBody::from).collect(),
    }))
}

async fn create<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, team)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(body): Json<DraftBody>,
) -> Result<(StatusCode, Json<ScheduleBody>), OncallRefused>
where
    S::Repos: OncallRepos,
{
    let token = carried(&state, &headers).await?;
    let draft = body.draft()?;
    let made = state
        .app
        .create_schedule(&token, TenantId::new(org), TeamId::new(team), &draft)
        .await?;
    tracing::info!(org = %org, schedule = %made.schedule.id, "an on-call schedule was created");
    Ok((StatusCode::CREATED, Json(ScheduleBody::from(&made))))
}

async fn one<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<ScheduleBody>, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let who = reading(&state, &headers).await?;
    let held = state
        .app
        .schedule(&who, TenantId::new(org), ScheduleId::new(id))
        .await?;
    Ok(Json(ScheduleBody::from(&held)))
}

async fn update<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(body): Json<PatchBody>,
) -> Result<Json<ScheduleBody>, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let token = carried(&state, &headers).await?;
    let patch = body.patch()?;
    let changed = state
        .app
        .update_schedule(&token, TenantId::new(org), ScheduleId::new(id), patch)
        .await?;
    Ok(Json(ScheduleBody::from(&changed)))
}

async fn remove<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let token = carried(&state, &headers).await?;
    state
        .app
        .delete_schedule(&token, TenantId::new(org), ScheduleId::new(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn now<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<NowBody>, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let who = reading(&state, &headers).await?;
    let now = state
        .app
        .on_call_now(&who, TenantId::new(org), ScheduleId::new(id))
        .await?;
    Ok(Json(NowBody::from(&now)))
}

async fn whether<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<Whether>,
    headers: HeaderMap,
) -> Result<Json<Whose>, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let who = reading(&state, &headers).await?;
    let Some(email) = asked.email else {
        let duties = state.app.on_call_duties(&who, TenantId::new(org)).await?;
        return Ok(Json(Whose::Everyone(Everyone {
            schedules: duties.iter().map(DutyBody::from).collect(),
        })));
    };
    let status = state
        .app
        .whether_on_call(&who, TenantId::new(org), &email)
        .await?;
    Ok(Json(Whose::One(WhetherBody::from(&status))))
}

async fn add_override<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<OverrideAsked>,
) -> Result<(StatusCode, Json<OverrideBody>), OncallRefused>
where
    S::Repos: OncallRepos,
{
    let token = carried(&state, &headers).await?;
    let cover = state
        .app
        .add_override(
            &token,
            TenantId::new(org),
            ScheduleId::new(id),
            AccountId::new(asked.account),
            asked.from_ms,
            asked.to_ms,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(OverrideBody::from(&cover))))
}

async fn remove_override<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id, cover)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let token = carried(&state, &headers).await?;
    state
        .app
        .remove_override(
            &token,
            TenantId::new(org),
            ScheduleId::new(id),
            OverrideId::new(cover),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn pages<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(listing): Query<PageListing>,
    headers: HeaderMap,
) -> Result<Json<PageList>, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let who = reading(&state, &headers).await?;
    let scope = match listing.state.as_deref() {
        None => PageScope::Open,
        Some(asked) => PageScope::parse(asked.trim()).ok_or(OncallRefused::Malformed("state"))?,
    };
    let held = state
        .app
        .list_pages(&who, TenantId::new(org), scope)
        .await?;
    Ok(Json(PageList {
        pages: held.iter().map(PageBody::from).collect(),
    }))
}

async fn acknowledge<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<PageBody>, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let who = reading(&state, &headers).await?;
    let acknowledged = state
        .app
        .acknowledge_page(&who, TenantId::new(org), PageId::new(id))
        .await?;
    Ok(Json(PageBody::from(&acknowledged)))
}

async fn acknowledge_link<S: Stack>(
    State(state): State<AppState<S>>,
    Query(link): Query<AckLink>,
) -> Result<Redirect, OncallRefused>
where
    S::Repos: OncallRepos,
{
    let page = state.app.acknowledge_page_link(link.token.trim()).await?;
    tracing::info!(page = %page.id, "a page was acknowledged by link");
    Ok(Redirect::to(&landing(&page)))
}

fn landing(page: &Page) -> String {
    match page.source {
        PageSource::Alert { rule, .. } => format!("/alerts?rule={rule}"),
        PageSource::Incident(incident) => format!("/incidents/{incident}"),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OncallRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error("the field {0} is not one this route understands")]
    Malformed(&'static str),
    #[error(transparent)]
    Schedule(#[from] BadSchedule),
    #[error(transparent)]
    Refused(OncallError),
}

impl From<OncallError> for OncallRefused {
    fn from(error: OncallError) -> Self {
        match error {
            OncallError::NoSession => Self::Reaching(NotCarried::NoSession),
            OncallError::Schedule(bad) => Self::Schedule(bad),
            other => Self::Refused(other),
        }
    }
}

fn status_of(error: OncallError) -> StatusCode {
    match error {
        OncallError::NoSession => StatusCode::UNAUTHORIZED,
        OncallError::NotYours
        | OncallError::NoSuchTeam
        | OncallError::NoSuchSchedule
        | OncallError::NoSuchOverride
        | OncallError::NoSuchPage
        | OncallError::NoAckKey => StatusCode::NOT_FOUND,
        OncallError::NotYoursToChange
        | OncallError::OutOfScope
        | OncallError::ReadOnly
        | OncallError::NotYourPage
        | OncallError::Ack(AckRefused::Tampered) => StatusCode::FORBIDDEN,
        OncallError::Ack(AckRefused::Expired) => StatusCode::GONE,
        OncallError::NotInTeam
        | OncallError::NotAMember
        | OncallError::NameTaken
        | OncallError::Schedule(_) => StatusCode::UNPROCESSABLE_ENTITY,
        OncallError::StoreUnreachable => StatusCode::SERVICE_UNAVAILABLE,
    }
}

impl IntoResponse for OncallRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Malformed(_) | Self::Schedule(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
            }
            Self::Refused(OncallError::NotYours) => {
                (StatusCode::NOT_FOUND, "no such organisation".to_owned())
            }
            Self::Refused(error) => (status_of(error), error.to_string()),
        };
        if status.is_server_error() {
            tracing::error!(%message, "the on-call routes could not be served");
        }
        (status, message).into_response()
    }
}
