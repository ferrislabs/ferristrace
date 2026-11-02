use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, patch, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::StatusRepos;
use ferristrace_domain::{
    Accent, BadStatusPage, Component, ComponentDraft, ComponentId, ComponentName, ComponentPatch,
    ComponentStatus, Email, IncidentId, Intro, Landing, Link, LinkRefused, Listed, Mode, PageDraft,
    PageName, PagePatch, ProjectId, PublicIncident, PublicView, RuleId, ServiceName, Sheet, Slug,
    StatusError, StatusPage, StatusPageId, TenantId, Theme,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::carried::{NotCarried, reading};
use crate::state::AppState;

const PUBLIC_CACHE: HeaderValue = HeaderValue::from_static("public, max-age=30");
const ANY_ORIGIN: HeaderValue = HeaderValue::from_static("*");

pub fn status_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: StatusRepos,
{
    Router::new()
        .route(
            "/api/orgs/{org}/status-pages",
            get(list::<S>).post(create::<S>),
        )
        .route(
            "/api/orgs/{org}/status-pages/{id}",
            get(one::<S>).patch(update::<S>).delete(remove::<S>),
        )
        .route(
            "/api/orgs/{org}/status-pages/{id}/preview",
            get(preview::<S>),
        )
        .route(
            "/api/orgs/{org}/status-pages/{id}/components",
            post(add_component::<S>),
        )
        .route(
            "/api/orgs/{org}/status-pages/{id}/components/order",
            post(order::<S>),
        )
        .route(
            "/api/orgs/{org}/status-pages/{id}/components/{component}",
            patch(change_component::<S>).delete(remove_component::<S>),
        )
        .route("/api/status/confirm", get(confirm::<S>))
        .route("/api/status/unsubscribe", get(unsubscribe::<S>))
        .route("/api/status/{slug}", get(public::<S>))
        .route("/api/status/{slug}/subscribe", post(subscribe::<S>))
        .route(
            "/api/status/{slug}/postmortems/{incident}",
            get(postmortem::<S>),
        )
        .with_state(state)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeBody {
    accent: String,
    mode: String,
}

impl ThemeBody {
    fn of(theme: Theme) -> Self {
        Self {
            accent: theme.accent.as_str().to_owned(),
            mode: theme.mode.as_str().to_owned(),
        }
    }

    fn theme(&self) -> Result<Theme, StatusRefused> {
        Ok(Theme {
            accent: Accent::parse(self.accent.trim()).ok_or(BadStatusPage::Accent)?,
            mode: Mode::parse(self.mode.trim()).ok_or(BadStatusPage::Mode)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkBody {
    Service {
        service: String,
        #[serde(default)]
        project: Option<Uuid>,
    },
    Alert {
        alert: Uuid,
    },
    Manual,
}

impl LinkBody {
    fn of(link: &Link) -> Self {
        match link {
            Link::Service { name, project } => Self::Service {
                service: name.as_str().to_owned(),
                project: project.map(ProjectId::as_uuid),
            },
            Link::Alert(rule) => Self::Alert {
                alert: rule.as_uuid(),
            },
            Link::Manual => Self::Manual,
        }
    }

    fn link(&self) -> Result<Link, StatusRefused> {
        Ok(match self {
            Self::Service { service, project } => Link::Service {
                name: ServiceName::try_from(service.as_str())?,
                project: project.map(ProjectId::new),
            },
            Self::Alert { alert } => Link::Alert(RuleId::new(*alert)),
            Self::Manual => Link::Manual,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct ComponentBody {
    id: Uuid,
    name: String,
    link: LinkBody,
    manual_status: Option<&'static str>,
    position: u32,
    public: bool,
}

impl From<&Component> for ComponentBody {
    fn from(component: &Component) -> Self {
        Self {
            id: component.id.as_uuid(),
            name: component.name.as_str().to_owned(),
            link: LinkBody::of(&component.link),
            manual_status: component.manual_status.map(ComponentStatus::as_str),
            position: component.position,
            public: component.public,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PageBody {
    id: Uuid,
    name: String,
    slug: String,
    public: bool,
    hide_from_search: bool,
    theme: ThemeBody,
    intro: Option<String>,
    subscribers: u64,
    created_ms: u64,
    updated_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    components: Option<Vec<ComponentBody>>,
}

impl PageBody {
    fn of(page: &StatusPage, subscribers: u64, components: Option<&[Component]>) -> Self {
        Self {
            id: page.id.as_uuid(),
            name: page.name.as_str().to_owned(),
            slug: page.slug.as_str().to_owned(),
            public: page.public,
            hide_from_search: page.hide_from_search,
            theme: ThemeBody::of(page.theme),
            intro: page.intro.as_ref().map(|intro| intro.as_str().to_owned()),
            subscribers,
            created_ms: page.created_ms,
            updated_ms: page.updated_ms,
            components: components
                .map(|components| components.iter().map(ComponentBody::from).collect()),
        }
    }
}

impl From<&Sheet> for PageBody {
    fn from(sheet: &Sheet) -> Self {
        Self::of(&sheet.page, sheet.subscribers, Some(&sheet.components))
    }
}

impl From<&Listed> for PageBody {
    fn from(listed: &Listed) -> Self {
        Self::of(&listed.page, listed.subscribers, None)
    }
}

#[derive(Debug, Serialize)]
pub struct PageList {
    status_pages: Vec<PageBody>,
}

#[derive(Debug, Serialize)]
pub struct ComponentList {
    components: Vec<ComponentBody>,
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
pub struct PageAsked {
    name: String,
    slug: String,
    #[serde(default)]
    public: bool,
    #[serde(default)]
    hide_from_search: bool,
    #[serde(default)]
    theme: Option<ThemeBody>,
    #[serde(default)]
    intro: Option<String>,
}

impl PageAsked {
    fn draft(&self) -> Result<PageDraft, StatusRefused> {
        Ok(PageDraft {
            name: PageName::try_from(self.name.as_str())?,
            slug: Slug::try_from(self.slug.as_str())?,
            public: self.public,
            hide_from_search: self.hide_from_search,
            theme: self
                .theme
                .as_ref()
                .map(ThemeBody::theme)
                .transpose()?
                .unwrap_or_default(),
            intro: self
                .intro
                .as_deref()
                .filter(|intro| !intro.trim().is_empty())
                .map(Intro::try_from)
                .transpose()?,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct PageChanged {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    public: Option<bool>,
    #[serde(default)]
    hide_from_search: Option<bool>,
    #[serde(default)]
    theme: Option<ThemeBody>,
    #[serde(default)]
    intro: Given<String>,
}

impl PageChanged {
    fn patch(&self) -> Result<PagePatch, StatusRefused> {
        Ok(PagePatch {
            name: self.name.as_deref().map(PageName::try_from).transpose()?,
            slug: self.slug.as_deref().map(Slug::try_from).transpose()?,
            public: self.public,
            hide_from_search: self.hide_from_search,
            theme: self.theme.as_ref().map(ThemeBody::theme).transpose()?,
            intro: match &self.intro {
                Given::Absent => None,
                Given::Cleared => Some(None),
                Given::Set(intro) if intro.trim().is_empty() => Some(None),
                Given::Set(intro) => Some(Some(Intro::try_from(intro.as_str())?)),
            },
        })
    }
}

fn manual_status(text: &str) -> Result<ComponentStatus, StatusRefused> {
    Ok(ComponentStatus::parse(text.trim()).ok_or(BadStatusPage::Status)?)
}

#[derive(Debug, Deserialize)]
pub struct ComponentAsked {
    name: String,
    link: LinkBody,
    #[serde(default)]
    manual_status: Option<String>,
    #[serde(default = "shown")]
    public: bool,
}

fn shown() -> bool {
    true
}

impl ComponentAsked {
    fn draft(&self) -> Result<ComponentDraft, StatusRefused> {
        Ok(ComponentDraft {
            name: ComponentName::try_from(self.name.as_str())?,
            link: self.link.link()?,
            manual_status: self
                .manual_status
                .as_deref()
                .map(manual_status)
                .transpose()?,
            public: self.public,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct ComponentChanged {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    link: Option<LinkBody>,
    #[serde(default)]
    manual_status: Given<String>,
    #[serde(default)]
    public: Option<bool>,
}

impl ComponentChanged {
    fn patch(&self) -> Result<ComponentPatch, StatusRefused> {
        Ok(ComponentPatch {
            name: self
                .name
                .as_deref()
                .map(ComponentName::try_from)
                .transpose()?,
            link: self.link.as_ref().map(LinkBody::link).transpose()?,
            manual_status: match &self.manual_status {
                Given::Absent => None,
                Given::Cleared => Some(None),
                Given::Set(status) => Some(Some(manual_status(status)?)),
            },
            public: self.public,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OrderAsked {
    Named { ids: Vec<Uuid> },
    Bare(Vec<Uuid>),
}

impl OrderAsked {
    fn ids(&self) -> Vec<ComponentId> {
        let ids = match self {
            Self::Named { ids } | Self::Bare(ids) => ids,
        };
        ids.iter().copied().map(ComponentId::new).collect()
    }
}

#[derive(Debug, Deserialize)]
pub struct SubscribeAsked {
    email: String,
}

#[derive(Debug, Deserialize)]
pub struct TokenLink {
    #[serde(default)]
    token: String,
}

#[derive(Debug, Serialize)]
pub struct PublicPageBody {
    name: String,
    theme: ThemeBody,
    intro: Option<String>,
    hide_from_search: bool,
}

#[derive(Debug, Serialize)]
pub struct PublicComponentBody {
    name: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct PublicUpdateBody {
    at_ms: u64,
    kind: &'static str,
    text: String,
}

#[derive(Debug, Serialize)]
pub struct PublicIncidentBody {
    title: String,
    status: &'static str,
    severity: &'static str,
    opened_ms: u64,
    resolved_ms: Option<u64>,
    updates: Vec<PublicUpdateBody>,
    postmortem_url: Option<String>,
}

impl PublicIncidentBody {
    fn of(incident: &PublicIncident, slug: &Slug) -> Self {
        Self {
            title: incident.title.as_str().to_owned(),
            status: incident.status.as_str(),
            severity: incident.severity.as_str(),
            opened_ms: incident.opened_ms,
            resolved_ms: incident.resolved_ms,
            updates: incident
                .updates
                .iter()
                .map(|update| PublicUpdateBody {
                    at_ms: update.at_ms,
                    kind: update.kind.public(),
                    text: update.text.to_string(),
                })
                .collect(),
            postmortem_url: incident
                .postmortem
                .then(|| format!("/status/{slug}/postmortems/{}", incident.id)),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PublicBody {
    page: PublicPageBody,
    overall: &'static str,
    components: Vec<PublicComponentBody>,
    incidents: Vec<PublicIncidentBody>,
}

impl From<&PublicView> for PublicBody {
    fn from(view: &PublicView) -> Self {
        Self {
            page: PublicPageBody {
                name: view.name.as_str().to_owned(),
                theme: ThemeBody::of(view.theme),
                intro: view.intro.as_ref().map(|intro| intro.as_str().to_owned()),
                hide_from_search: view.hide_from_search,
            },
            overall: view.overall.as_str(),
            components: view
                .components
                .iter()
                .map(|component| PublicComponentBody {
                    name: component.name.as_str().to_owned(),
                    status: component.status.as_str(),
                })
                .collect(),
            incidents: view
                .incidents
                .iter()
                .map(|incident| PublicIncidentBody::of(incident, &view.slug))
                .collect(),
        }
    }
}

fn shared(body: Value) -> Response {
    let mut response = Json(body).into_response();
    let headers = response.headers_mut();
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, ANY_ORIGIN);
    headers.insert(CACHE_CONTROL, PUBLIC_CACHE);
    response
}

fn slug_of(text: &str) -> Result<Slug, StatusRefused> {
    Slug::try_from(text).map_err(|_| StatusRefused::Refused(StatusError::NoSuchPage))
}

async fn list<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<PageList>, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    let listed = state.app.status_pages(&who, TenantId::new(org)).await?;
    Ok(Json(PageList {
        status_pages: listed.iter().map(PageBody::from).collect(),
    }))
}

async fn one<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<PageBody>, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    let sheet = state
        .app
        .status_page(&who, TenantId::new(org), StatusPageId::new(id))
        .await?;
    Ok(Json(PageBody::from(&sheet)))
}

async fn create<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<PageAsked>,
) -> Result<(StatusCode, Json<PageBody>), StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    let draft = asked.draft()?;
    let sheet = state
        .app
        .create_status_page(&who, TenantId::new(org), &draft)
        .await?;
    tracing::info!(org = %org, page = %sheet.page.id, "a status page was created");
    Ok((StatusCode::CREATED, Json(PageBody::from(&sheet))))
}

async fn update<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<PageChanged>,
) -> Result<Json<PageBody>, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    let patch = asked.patch()?;
    let sheet = state
        .app
        .update_status_page(&who, TenantId::new(org), StatusPageId::new(id), patch)
        .await?;
    Ok(Json(PageBody::from(&sheet)))
}

async fn remove<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    state
        .app
        .delete_status_page(&who, TenantId::new(org), StatusPageId::new(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn preview<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<PublicBody>, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    let view = state
        .app
        .preview_status_page(&who, TenantId::new(org), StatusPageId::new(id))
        .await?;
    Ok(Json(PublicBody::from(&view)))
}

async fn add_component<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<ComponentAsked>,
) -> Result<(StatusCode, Json<ComponentBody>), StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    let draft = asked.draft()?;
    let component = state
        .app
        .add_status_component(&who, TenantId::new(org), StatusPageId::new(id), &draft)
        .await?;
    Ok((StatusCode::CREATED, Json(ComponentBody::from(&component))))
}

async fn change_component<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id, component)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<ComponentChanged>,
) -> Result<Json<ComponentBody>, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    let patch = asked.patch()?;
    let changed = state
        .app
        .change_status_component(
            &who,
            TenantId::new(org),
            StatusPageId::new(id),
            ComponentId::new(component),
            patch,
        )
        .await?;
    Ok(Json(ComponentBody::from(&changed)))
}

async fn remove_component<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id, component)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    state
        .app
        .remove_status_component(
            &who,
            TenantId::new(org),
            StatusPageId::new(id),
            ComponentId::new(component),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn order<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<OrderAsked>,
) -> Result<Json<ComponentList>, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let who = reading(&headers)?;
    let ordered = state
        .app
        .order_status_components(
            &who,
            TenantId::new(org),
            StatusPageId::new(id),
            &asked.ids(),
        )
        .await?;
    Ok(Json(ComponentList {
        components: ordered.iter().map(ComponentBody::from).collect(),
    }))
}

async fn public<S: Stack>(
    State(state): State<AppState<S>>,
    Path(slug): Path<String>,
) -> Result<Response, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let slug = slug_of(&slug)?;
    let view = state.app.public_status(&slug).await?;
    Ok(shared(json!(PublicBody::from(&view))))
}

async fn postmortem<S: Stack>(
    State(state): State<AppState<S>>,
    Path((slug, incident)): Path<(String, Uuid)>,
) -> Result<Response, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let slug = slug_of(&slug)?;
    let held = state
        .app
        .public_postmortem(&slug, IncidentId::new(incident))
        .await?;
    Ok(shared(json!({
        "title": held.title.as_str(),
        "body": &*held.body,
        "published_ms": held.published_ms,
    })))
}

async fn subscribe<S: Stack>(
    State(state): State<AppState<S>>,
    Path(slug): Path<String>,
    Json(asked): Json<SubscribeAsked>,
) -> Result<StatusCode, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let email =
        Email::try_from(asked.email.as_str()).map_err(|_| StatusRefused::Malformed("email"))?;
    let Ok(slug) = Slug::try_from(slug.as_str()) else {
        return Ok(StatusCode::ACCEPTED);
    };
    if state.app.subscribe_to_status(&slug, &email).await? {
        tracing::info!(slug = %slug, "a status page subscription was asked for");
    }
    Ok(StatusCode::ACCEPTED)
}

fn landed(landing: &Landing) -> Redirect {
    Redirect::to(&format!(
        "/status/{}?notice={}",
        landing.slug,
        landing.notice.as_str()
    ))
}

async fn confirm<S: Stack>(
    State(state): State<AppState<S>>,
    Query(link): Query<TokenLink>,
) -> Result<Redirect, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let landing = state
        .app
        .confirm_status_subscription(link.token.trim())
        .await?;
    Ok(landed(&landing))
}

async fn unsubscribe<S: Stack>(
    State(state): State<AppState<S>>,
    Query(link): Query<TokenLink>,
) -> Result<Redirect, StatusRefused>
where
    S::Repos: StatusRepos,
{
    let landing = state.app.unsubscribe_from_status(link.token.trim()).await?;
    Ok(landed(&landing))
}

#[derive(Debug, thiserror::Error)]
pub enum StatusRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error("the field {0} is not one this route understands")]
    Malformed(&'static str),
    #[error(transparent)]
    Page(#[from] BadStatusPage),
    #[error(transparent)]
    Refused(StatusError),
}

impl From<StatusError> for StatusRefused {
    fn from(error: StatusError) -> Self {
        match error {
            StatusError::NoSession => Self::Reaching(NotCarried::NoSession),
            StatusError::Page(bad) => Self::Page(bad),
            other => Self::Refused(other),
        }
    }
}

fn status_of(error: StatusError) -> StatusCode {
    match error {
        StatusError::NoSession => StatusCode::UNAUTHORIZED,
        StatusError::NotYours
        | StatusError::NoSuchPage
        | StatusError::NoSuchComponent
        | StatusError::NoSuchPostmortem
        | StatusError::NoSuchSubscriber => StatusCode::NOT_FOUND,
        StatusError::ReadOnly
        | StatusError::NotAdmin
        | StatusError::Link(LinkRefused::Tampered) => StatusCode::FORBIDDEN,
        StatusError::Link(LinkRefused::Expired) => StatusCode::GONE,
        StatusError::SlugTaken => StatusCode::CONFLICT,
        StatusError::NoSuchAlert
        | StatusError::NoSuchProject
        | StatusError::TooManyComponents { .. }
        | StatusError::Order
        | StatusError::Page(_) => StatusCode::UNPROCESSABLE_ENTITY,
        StatusError::StoreUnreachable => StatusCode::SERVICE_UNAVAILABLE,
    }
}

impl IntoResponse for StatusRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Malformed(_) | Self::Page(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
            }
            Self::Refused(StatusError::NotYours) => {
                (StatusCode::NOT_FOUND, "no such organisation".to_owned())
            }
            Self::Refused(error) => (status_of(error), error.to_string()),
        };
        if status.is_server_error() {
            tracing::error!(%message, "the status pages could not be served");
        }
        (status, message).into_response()
    }
}
