use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::IncidentRepos;
use ferristrace_domain::{
    AccountId, AlertRef, Attachment, AttachmentKind, AttachmentLabel, Author, BadIncident, Briefed,
    Change, Chronicle, CommentBody, Dossier, IncidentDraft, IncidentError, IncidentFilter,
    IncidentId, IncidentPatch, IncidentScope, IncidentSeverity, IncidentStatus, IncidentTitle,
    Person, ProjectId, Reference, RuleId, ScheduleId, TenantId, TimelineEntry,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::carried::{NotCarried, reading};
use crate::context_body::ContextBody;
use crate::state::AppState;

pub fn incident_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: IncidentRepos,
{
    Router::new()
        .route("/api/orgs/{org}/incidents", get(list::<S>).post(open::<S>))
        .route(
            "/api/orgs/{org}/incidents/{id}",
            get(one::<S>).patch(amend::<S>),
        )
        .route(
            "/api/orgs/{org}/incidents/{id}/timeline",
            get(timeline::<S>),
        )
        .route(
            "/api/orgs/{org}/incidents/{id}/comments",
            post(comment::<S>),
        )
        .route(
            "/api/orgs/{org}/incidents/{id}/attachments",
            post(attach::<S>),
        )
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct PersonBody {
    id: Uuid,
    email: Option<String>,
    name: Option<String>,
}

impl PersonBody {
    fn of(person: &Person) -> Self {
        Self {
            id: person.account.as_uuid(),
            email: person.email.as_ref().map(|email| email.as_str().to_owned()),
            name: person.name.as_deref().map(ToOwned::to_owned),
        }
    }

    fn known(chronicle: &Chronicle, account: AccountId) -> Self {
        chronicle.person(account).map_or(
            Self {
                id: account.as_uuid(),
                email: None,
                name: None,
            },
            Self::of,
        )
    }
}

#[derive(Debug, Serialize)]
struct AlertBody {
    id: Uuid,
    name: String,
}

impl From<&AlertRef> for AlertBody {
    fn from(alert: &AlertRef) -> Self {
        Self {
            id: alert.id.as_uuid(),
            name: alert.name.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct IncidentBody {
    id: Uuid,
    title: String,
    severity: &'static str,
    status: &'static str,
    owner: Option<PersonBody>,
    public: bool,
    project: Option<Uuid>,
    opened_ms: u64,
    resolved_ms: Option<u64>,
    alert: Option<AlertBody>,
    paged_schedule: Option<Uuid>,
}

impl From<&Briefed> for IncidentBody {
    fn from(briefed: &Briefed) -> Self {
        let incident = &briefed.incident;
        Self {
            id: incident.id.as_uuid(),
            title: incident.title.as_str().to_owned(),
            severity: incident.severity.as_str(),
            status: incident.status.as_str(),
            owner: briefed.owner.as_ref().map(PersonBody::of),
            public: incident.public,
            project: incident.project.map(ProjectId::as_uuid),
            opened_ms: incident.opened_ms,
            resolved_ms: incident.resolved_ms,
            alert: briefed.alert.as_ref().map(AlertBody::from),
            paged_schedule: incident.paged_schedule.map(ScheduleId::as_uuid),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct IncidentList {
    incidents: Vec<IncidentBody>,
}

#[derive(Debug, Serialize)]
pub struct AttachmentBody {
    id: Uuid,
    kind: &'static str,
    reference: String,
    label: String,
    at_ms: u64,
}

impl From<&Attachment> for AttachmentBody {
    fn from(attachment: &Attachment) -> Self {
        Self {
            id: attachment.id.as_uuid(),
            kind: attachment.kind().as_str(),
            reference: attachment.reference.as_str().to_owned(),
            label: attachment.label.as_str().to_owned(),
            at_ms: attachment.at_ms,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuthorBody {
    kind: &'static str,
    id: Option<Uuid>,
    label: String,
}

#[derive(Debug, Serialize)]
pub struct EntryBody {
    at_ms: u64,
    kind: &'static str,
    author: AuthorBody,
    detail: Value,
}

fn alert_of(chronicle: &Chronicle, rule: RuleId) -> Value {
    json!({
        "id": rule.as_uuid(),
        "name": chronicle.alert(rule).map(|alert| alert.name.to_string()),
    })
}

fn owner_of(chronicle: &Chronicle, account: Option<AccountId>) -> Value {
    account.map_or(Value::Null, |account| {
        json!(PersonBody::known(chronicle, account))
    })
}

fn detail_of(chronicle: &Chronicle, change: &Change) -> Value {
    match change {
        Change::Opened {
            title,
            severity,
            alert,
        } => json!({
            "title": title.as_str(),
            "severity": severity.as_str(),
            "alert": alert.map_or(Value::Null, |rule| alert_of(chronicle, rule)),
        }),
        Change::Status { from, to } => json!({"from": from.as_str(), "to": to.as_str()}),
        Change::Severity { from, to } => json!({"from": from.as_str(), "to": to.as_str()}),
        Change::Owner { from, to } => json!({
            "from": owner_of(chronicle, *from),
            "to": owner_of(chronicle, *to),
        }),
        Change::Title { from, to } => json!({"from": from.as_str(), "to": to.as_str()}),
        Change::Public { from, to } => json!({"from": from, "to": to}),
        Change::Comment { body, public } => json!({"body": body.as_str(), "public": public}),
        Change::Alert {
            rule,
            from,
            to,
            value,
            labels,
            context,
        } => {
            let mut detail = json!({
                "alert": alert_of(chronicle, *rule),
                "from": from.as_str(),
                "to": to.as_str(),
                "value": value,
                "labels": labels
                    .pairs()
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect::<BTreeMap<_, _>>(),
            });
            if let (Some(context), Some(fields)) = (context, detail.as_object_mut()) {
                fields.insert("context".to_owned(), json!(ContextBody::from(context)));
            }
            detail
        }
        Change::Attachment(attachment) => json!(AttachmentBody::from(attachment)),
        Change::Paged { page, step, target } => json!({
            "page": page.as_uuid(),
            "step": step,
            "target": owner_of(chronicle, *target),
        }),
        Change::Acknowledged { page } => json!({"page": page.as_uuid()}),
        Change::Deploy {
            service,
            version,
            environment,
        } => json!({
            "service": service,
            "version": version,
            "environment": environment,
        }),
    }
}

fn entry_of(chronicle: &Chronicle, entry: &TimelineEntry) -> EntryBody {
    let id = match &entry.author {
        Author::Account(account) => Some(account.as_uuid()),
        Author::ApiKey(key) => Some(key.as_uuid()),
        Author::Agent(_) | Author::System => None,
    };
    EntryBody {
        at_ms: entry.at_ms,
        kind: entry.change.kind().as_str(),
        author: AuthorBody {
            kind: entry.author.kind(),
            id,
            label: chronicle.author_label(&entry.author).to_owned(),
        },
        detail: detail_of(chronicle, &entry.change),
    }
}

fn timeline_of(chronicle: &Chronicle) -> Vec<EntryBody> {
    chronicle
        .entries
        .iter()
        .map(|entry| entry_of(chronicle, entry))
        .collect()
}

#[derive(Debug, Serialize)]
pub struct DossierBody {
    #[serde(flatten)]
    incident: IncidentBody,
    attachments: Vec<AttachmentBody>,
    timeline: Vec<EntryBody>,
}

impl From<&Dossier> for DossierBody {
    fn from(dossier: &Dossier) -> Self {
        Self {
            incident: IncidentBody::from(&dossier.briefed),
            attachments: dossier
                .attachments
                .iter()
                .map(AttachmentBody::from)
                .collect(),
            timeline: timeline_of(&dossier.chronicle),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TimelineBody {
    timeline: Vec<EntryBody>,
}

#[derive(Debug, Deserialize)]
pub struct Listing {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    project: Option<Uuid>,
}

#[derive(Debug, Default)]
enum Given<T> {
    #[default]
    Absent,
    Cleared,
    Set(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Given<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Option::<T>::deserialize(deserializer)?.map_or(Self::Cleared, Self::Set))
    }
}

#[derive(Debug, Deserialize)]
pub struct DraftBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    alert: Option<Uuid>,
    #[serde(default)]
    public: bool,
    #[serde(default)]
    page: Option<PagedBody>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PagedBody {
    schedule: Uuid,
}

impl DraftBody {
    fn draft(&self) -> Result<IncidentDraft, IncidentRefused> {
        Ok(IncidentDraft {
            title: self
                .title
                .as_deref()
                .map(IncidentTitle::try_from)
                .transpose()?,
            severity: self.severity.as_deref().map(severity).transpose()?,
            project: self.project.map(ProjectId::new),
            alert: self.alert.map(RuleId::new),
            public: self.public,
            page: self.page.map(|page| ScheduleId::new(page.schedule)),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct PatchBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    owner: Given<Uuid>,
    #[serde(default)]
    public: Option<bool>,
}

impl PatchBody {
    fn patch(&self) -> Result<IncidentPatch, IncidentRefused> {
        Ok(IncidentPatch {
            title: self
                .title
                .as_deref()
                .map(IncidentTitle::try_from)
                .transpose()?,
            severity: self.severity.as_deref().map(severity).transpose()?,
            status: self
                .status
                .as_deref()
                .map(|status| {
                    IncidentStatus::parse(status.trim()).ok_or(IncidentRefused::Malformed("status"))
                })
                .transpose()?,
            owner: match self.owner {
                Given::Absent => None,
                Given::Cleared => Some(None),
                Given::Set(owner) => Some(Some(AccountId::new(owner))),
            },
            public: self.public,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct CommentAsked {
    body: String,
    #[serde(default)]
    public: bool,
}

#[derive(Debug, Deserialize)]
pub struct AttachmentAsked {
    kind: String,
    reference: String,
    label: String,
}

fn severity(text: &str) -> Result<IncidentSeverity, IncidentRefused> {
    IncidentSeverity::parse(text.trim()).ok_or(IncidentRefused::Malformed("severity"))
}

async fn list<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(listing): Query<Listing>,
    headers: HeaderMap,
) -> Result<Json<IncidentList>, IncidentRefused>
where
    S::Repos: IncidentRepos,
{
    let who = reading(&state, &headers).await?;
    let scope = match listing.status.as_deref() {
        None => IncidentScope::Open,
        Some(asked) => {
            IncidentScope::parse(asked.trim()).ok_or(IncidentRefused::Malformed("status"))?
        }
    };
    let found = state
        .app
        .list_incidents(
            &who,
            TenantId::new(org),
            IncidentFilter {
                scope,
                project: listing.project.map(ProjectId::new),
            },
        )
        .await?;
    Ok(Json(IncidentList {
        incidents: found.iter().map(IncidentBody::from).collect(),
    }))
}

async fn one<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<DossierBody>, IncidentRefused>
where
    S::Repos: IncidentRepos,
{
    let who = reading(&state, &headers).await?;
    let dossier = state
        .app
        .incident(&who, TenantId::new(org), IncidentId::new(id))
        .await?;
    Ok(Json(DossierBody::from(&dossier)))
}

async fn timeline<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<TimelineBody>, IncidentRefused>
where
    S::Repos: IncidentRepos,
{
    let who = reading(&state, &headers).await?;
    let chronicle = state
        .app
        .incident_timeline(&who, TenantId::new(org), IncidentId::new(id))
        .await?;
    Ok(Json(TimelineBody {
        timeline: timeline_of(&chronicle),
    }))
}

async fn open<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<DraftBody>,
) -> Result<(StatusCode, Json<IncidentBody>), IncidentRefused>
where
    S::Repos: IncidentRepos,
{
    let who = reading(&state, &headers).await?;
    let draft = body.draft()?;
    let opened = state
        .app
        .open_incident(&who, TenantId::new(org), &draft)
        .await?;
    tracing::info!(org = %org, incident = %opened.incident.id, "an incident was opened");
    Ok((StatusCode::CREATED, Json(IncidentBody::from(&opened))))
}

async fn amend<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(body): Json<PatchBody>,
) -> Result<Json<IncidentBody>, IncidentRefused>
where
    S::Repos: IncidentRepos,
{
    let who = reading(&state, &headers).await?;
    let patch = body.patch()?;
    let amended = state
        .app
        .amend_incident(&who, TenantId::new(org), IncidentId::new(id), patch)
        .await?;
    Ok(Json(IncidentBody::from(&amended)))
}

async fn comment<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<CommentAsked>,
) -> Result<(StatusCode, Json<EntryBody>), IncidentRefused>
where
    S::Repos: IncidentRepos,
{
    let who = reading(&state, &headers).await?;
    let body = CommentBody::try_from(asked.body.as_str())?;
    let chronicle = state
        .app
        .comment_on_incident(
            &who,
            TenantId::new(org),
            IncidentId::new(id),
            &body,
            asked.public,
        )
        .await?;
    let entry = chronicle
        .entries
        .last()
        .map(|entry| entry_of(&chronicle, entry))
        .ok_or(IncidentRefused::Refused(IncidentError::NoSuchIncident))?;
    Ok((StatusCode::CREATED, Json(entry)))
}

async fn attach<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<AttachmentAsked>,
) -> Result<(StatusCode, Json<AttachmentBody>), IncidentRefused>
where
    S::Repos: IncidentRepos,
{
    let who = reading(&state, &headers).await?;
    let kind =
        AttachmentKind::parse(asked.kind.trim()).ok_or(IncidentRefused::Malformed("kind"))?;
    let reference = Reference::parse(kind, &asked.reference)?;
    let label = AttachmentLabel::try_from(asked.label.as_str())?;
    let attachment = state
        .app
        .attach_to_incident(
            &who,
            TenantId::new(org),
            IncidentId::new(id),
            reference,
            label,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(AttachmentBody::from(&attachment))))
}

#[derive(Debug, thiserror::Error)]
pub enum IncidentRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error("the field {0} is not one this route understands")]
    Malformed(&'static str),
    #[error(transparent)]
    Incident(#[from] BadIncident),
    #[error(transparent)]
    Refused(IncidentError),
}

impl From<IncidentError> for IncidentRefused {
    fn from(error: IncidentError) -> Self {
        match error {
            IncidentError::NoSession => Self::Reaching(NotCarried::NoSession),
            IncidentError::Incident(bad) => Self::Incident(bad),
            other => Self::Refused(other),
        }
    }
}

fn status_of(error: IncidentError) -> StatusCode {
    match error {
        IncidentError::NoSession => StatusCode::UNAUTHORIZED,
        IncidentError::NotYours | IncidentError::NoSuchIncident => StatusCode::NOT_FOUND,
        IncidentError::ReadOnly | IncidentError::OutOfScope => StatusCode::FORBIDDEN,
        IncidentError::NoSuchProject
        | IncidentError::NoSuchSchedule
        | IncidentError::NoSuchAlert
        | IncidentError::AlertNotFiring
        | IncidentError::NotAMember
        | IncidentError::Incident(_) => StatusCode::UNPROCESSABLE_ENTITY,
        IncidentError::StoreUnreachable => StatusCode::SERVICE_UNAVAILABLE,
    }
}

impl IntoResponse for IncidentRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Malformed(_) | Self::Incident(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
            }
            Self::Refused(IncidentError::NotYours) => {
                (StatusCode::NOT_FOUND, "no such organisation".to_owned())
            }
            Self::Refused(error) => (status_of(error), error.to_string()),
        };
        if status.is_server_error() {
            tracing::error!(%message, "the incidents could not be served");
        }
        (status, message).into_response()
    }
}
