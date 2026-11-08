use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::AlertRepos;
use ferristrace_domain::{
    Aggregation, AlertError, AlertEvent, AlertSeverity, AlertStatus, BadChannel, BadDesign,
    BadRule, ChannelChange, ChannelId, ChannelKind, ChannelName, ChannelView, Chat, Comparison,
    Condition, Definition, DefinitionParts, Delivery, Destination, IssuedChannel, Labels, LlmGroup,
    LlmMeasure, LlmSignal, LogSignal, MetricSignal, NoDataPolicy, Preview, ProjectId, RuleDraft,
    RuleId, RuleName, RulePatch, ScheduleId, SeriesState, Severity, Signal, TenantId, TraceMeasure,
    TraceSignal, Watched,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::carried::{NotCarried, carried, reading};
use crate::context_body::ContextBody;
use crate::design_body::DesignBody;
use crate::placement::PlacementBody;
use crate::state::AppState;

pub fn alert_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: AlertRepos,
{
    Router::new()
        .route("/api/orgs/{org}/alerts", get(list::<S>).post(create::<S>))
        .route("/api/orgs/{org}/alerts/preview", post(preview::<S>))
        .route(
            "/api/orgs/{org}/alerts/{id}",
            get(one::<S>).patch(update::<S>).delete(delete::<S>),
        )
        .route("/api/orgs/{org}/alerts/{id}/test", post(test::<S>))
        .route(
            "/api/orgs/{org}/alerts/{id}/silence",
            post(silence::<S>).delete(unsilence::<S>),
        )
        .route("/api/orgs/{org}/alerts/{id}/history", get(history::<S>))
        .route(
            "/api/orgs/{org}/channels",
            get(channels::<S>).post(create_channel::<S>),
        )
        .route(
            "/api/orgs/{org}/channels/preview",
            post(preview_channel::<S>),
        )
        .route(
            "/api/orgs/{org}/channels/{id}",
            patch(update_channel::<S>).delete(delete_channel::<S>),
        )
        .with_state(state)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SignalBody {
    Traces {
        #[serde(default)]
        service: Option<String>,
        #[serde(default)]
        operation: Option<String>,
        #[serde(default)]
        environment: Option<String>,
        measure: String,
        #[serde(default, skip_serializing_if = "PlacementBody::is_empty")]
        placement: PlacementBody,
    },
    Logs {
        #[serde(default)]
        services: Vec<String>,
        #[serde(default)]
        severities: Vec<String>,
        #[serde(default)]
        q: Option<String>,
        #[serde(default)]
        attrs: Vec<String>,
        #[serde(default = "counted")]
        measure: String,
        #[serde(default, skip_serializing_if = "PlacementBody::is_empty")]
        placement: PlacementBody,
    },
    Metrics {
        name: String,
        aggregation: String,
        #[serde(default)]
        group_by: Vec<String>,
        #[serde(default)]
        filter: BTreeMap<String, String>,
    },
    Llm {
        measure: String,
        #[serde(default = "ungrouped")]
        group_by: String,
        #[serde(default)]
        filter: LlmFilterBody,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LlmFilterBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    feature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    customer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<Uuid>,
}

fn ungrouped() -> String {
    LlmGroup::None.as_str().to_owned()
}

fn counted() -> String {
    "count".to_owned()
}

fn given(text: Option<String>) -> Option<Box<str>> {
    text.map(|held| held.trim().to_owned())
        .filter(|held| !held.is_empty())
        .map(Into::into)
}

impl SignalBody {
    fn of(signal: &Signal) -> Self {
        match signal {
            Signal::Traces(traces) => Self::Traces {
                service: traces.service.as_deref().map(ToOwned::to_owned),
                operation: traces.operation.as_deref().map(ToOwned::to_owned),
                environment: traces.environment.as_deref().map(ToOwned::to_owned),
                measure: traces.measure.as_str().to_owned(),
                placement: PlacementBody::of(&traces.placement),
            },
            Signal::Logs(logs) => Self::Logs {
                services: logs.services.iter().map(ToString::to_string).collect(),
                severities: logs
                    .severities
                    .iter()
                    .map(|severity| severity.name().to_owned())
                    .collect(),
                q: logs.text.as_deref().map(ToOwned::to_owned),
                attrs: logs
                    .attributes
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect(),
                measure: counted(),
                placement: PlacementBody::of(&logs.placement),
            },
            Signal::Metrics(metric) => Self::Metrics {
                name: metric.name.to_string(),
                aggregation: metric.aggregation.name().to_owned(),
                group_by: metric.group_by.iter().map(ToString::to_string).collect(),
                filter: metric
                    .filter
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            },
            Signal::Llm(llm) => Self::Llm {
                measure: llm.measure.as_str().to_owned(),
                group_by: llm.group_by.as_str().to_owned(),
                filter: LlmFilterBody {
                    feature: llm.feature.as_deref().map(ToOwned::to_owned),
                    customer: llm.customer.as_deref().map(ToOwned::to_owned),
                    model: llm.model.as_deref().map(ToOwned::to_owned),
                    project: llm.project.map(ProjectId::as_uuid),
                },
            },
        }
    }

    fn signal(self) -> Result<Signal, AlertRefused> {
        match self {
            Self::Traces {
                service,
                operation,
                environment,
                measure,
                placement,
            } => Ok(Signal::Traces(TraceSignal {
                service: given(service),
                operation: given(operation),
                environment: given(environment),
                measure: TraceMeasure::parse(&measure).ok_or(AlertRefused::Malformed("measure"))?,
                placement: placement
                    .placement()
                    .map_err(|_| AlertRefused::Malformed("placement"))?,
            })),
            Self::Logs {
                services,
                severities,
                q,
                attrs,
                measure,
                placement,
            } => {
                if measure != "count" {
                    return Err(AlertRefused::Malformed("measure"));
                }
                Ok(Signal::Logs(LogSignal {
                    services: services
                        .into_iter()
                        .filter_map(|service| given(Some(service)))
                        .collect(),
                    severities: severities
                        .iter()
                        .map(|severity| {
                            Severity::try_from(severity.trim())
                                .map_err(|_| AlertRefused::Malformed("severities"))
                        })
                        .collect::<Result<_, _>>()?,
                    text: given(q),
                    attributes: attrs
                        .iter()
                        .map(|pair| {
                            pair.split_once('=')
                                .filter(|(key, _)| !key.trim().is_empty())
                                .map(|(key, value)| (key.trim().into(), value.into()))
                                .ok_or(AlertRefused::Malformed("attrs"))
                        })
                        .collect::<Result<_, _>>()?,
                    placement: placement
                        .placement()
                        .map_err(|_| AlertRefused::Malformed("placement"))?,
                }))
            }
            Self::Metrics {
                name,
                aggregation,
                group_by,
                filter,
            } => Ok(Signal::Metrics(MetricSignal {
                name: name.trim().into(),
                aggregation: Aggregation::try_from(aggregation.as_str())
                    .map_err(|_| AlertRefused::Malformed("aggregation"))?,
                group_by: group_by.into_iter().map(|key| key.trim().into()).collect(),
                filter: filter
                    .into_iter()
                    .map(|(key, value)| (key.trim().into(), value.into()))
                    .collect(),
            })),
            Self::Llm {
                measure,
                group_by,
                filter,
            } => Ok(Signal::Llm(LlmSignal {
                measure: LlmMeasure::parse(measure.trim())
                    .ok_or(AlertRefused::Malformed("measure"))?,
                group_by: LlmGroup::parse(group_by.trim())
                    .ok_or(AlertRefused::Malformed("group_by"))?,
                feature: filter.feature.map(|held| held.trim().into()),
                customer: filter.customer.map(|held| held.trim().into()),
                model: filter.model.map(|held| held.trim().into()),
                project: filter.project.map(ProjectId::new),
            })),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ConditionBody {
    op: String,
    threshold: f64,
}

impl ConditionBody {
    fn condition(&self) -> Result<Condition, AlertRefused> {
        let op = Comparison::parse(self.op.trim()).ok_or(AlertRefused::Malformed("condition"))?;
        Ok(Condition::new(op, self.threshold)?)
    }
}

fn yes() -> bool {
    true
}

fn warning() -> String {
    AlertSeverity::Warning.as_str().to_owned()
}

fn quiet() -> String {
    NoDataPolicy::Ok.as_str().to_owned()
}

#[derive(Debug, Deserialize)]
pub struct RuleBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default = "yes")]
    enabled: bool,
    signal: SignalBody,
    condition: ConditionBody,
    window_s: u64,
    #[serde(default)]
    pending_s: u64,
    #[serde(default = "warning")]
    severity: String,
    #[serde(default = "quiet")]
    no_data: String,
    #[serde(default)]
    channels: Vec<Uuid>,
    #[serde(default)]
    page: Option<PageBody>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PageBody {
    schedule: Uuid,
}

impl PageBody {
    fn schedule(self) -> ScheduleId {
        ScheduleId::new(self.schedule)
    }
}

fn severity_of(text: &str) -> Result<AlertSeverity, AlertRefused> {
    AlertSeverity::parse(text.trim()).ok_or(AlertRefused::Malformed("severity"))
}

fn no_data_of(text: &str) -> Result<NoDataPolicy, AlertRefused> {
    NoDataPolicy::parse(text.trim()).ok_or(AlertRefused::Malformed("no_data"))
}

fn channels_of(ids: Vec<Uuid>) -> Vec<ChannelId> {
    ids.into_iter().map(ChannelId::new).collect()
}

impl RuleBody {
    fn definition(
        self,
    ) -> Result<(Option<String>, Option<ProjectId>, bool, Definition), AlertRefused> {
        let parts = DefinitionParts {
            condition: self.condition.condition()?,
            signal: self.signal.signal()?,
            window_secs: self.window_s,
            pending_secs: self.pending_s,
            severity: severity_of(&self.severity)?,
            no_data: no_data_of(&self.no_data)?,
            channels: channels_of(self.channels),
            page: self.page.map(PageBody::schedule),
        };
        Ok((
            self.name,
            self.project.map(ProjectId::new),
            self.enabled,
            Definition::new(parts)?,
        ))
    }

    fn draft(self) -> Result<RuleDraft, AlertRefused> {
        let (name, project, enabled, definition) = self.definition()?;
        Ok(RuleDraft {
            name: RuleName::try_from(name.as_deref().unwrap_or_default())?,
            project,
            enabled,
            definition,
        })
    }
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
pub struct RulePatchBody {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    project: Given<Uuid>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    signal: Option<SignalBody>,
    #[serde(default)]
    condition: Option<ConditionBody>,
    #[serde(default)]
    window_s: Option<u64>,
    #[serde(default)]
    pending_s: Option<u64>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    no_data: Option<String>,
    #[serde(default)]
    channels: Option<Vec<Uuid>>,
    #[serde(default)]
    page: Given<PageBody>,
}

impl RulePatchBody {
    fn patch(self) -> Result<RulePatch, AlertRefused> {
        Ok(RulePatch {
            name: self.name.as_deref().map(RuleName::try_from).transpose()?,
            project: match self.project {
                Given::Absent => None,
                Given::Cleared => Some(None),
                Given::Set(project) => Some(Some(ProjectId::new(project))),
            },
            enabled: self.enabled,
            signal: self.signal.map(SignalBody::signal).transpose()?,
            condition: self
                .condition
                .as_ref()
                .map(ConditionBody::condition)
                .transpose()?,
            window_secs: self.window_s,
            pending_secs: self.pending_s,
            severity: self.severity.as_deref().map(severity_of).transpose()?,
            no_data: self.no_data.as_deref().map(no_data_of).transpose()?,
            channels: self.channels.map(channels_of),
            page: match self.page {
                Given::Absent => None,
                Given::Cleared => Some(None),
                Given::Set(page) => Some(Some(page.schedule())),
            },
        })
    }
}

fn labelled(labels: &Labels) -> BTreeMap<String, String> {
    labels
        .pairs()
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[derive(Debug, Serialize)]
struct SeriesBody {
    labels: BTreeMap<String, String>,
    status: &'static str,
    value: Option<f64>,
}

#[derive(Debug, Serialize)]
struct StateBody {
    status: &'static str,
    value: Option<f64>,
    since_ms: u64,
    evaluated_ms: Option<u64>,
    series: Vec<SeriesBody>,
}

#[derive(Debug, Serialize)]
pub struct RuleView {
    id: Uuid,
    name: String,
    project: Option<Uuid>,
    enabled: bool,
    silenced_until_ms: Option<u64>,
    signal: SignalBody,
    condition: ConditionBody,
    window_s: u64,
    pending_s: u64,
    severity: &'static str,
    no_data: &'static str,
    channels: Vec<Uuid>,
    page: Option<PageBody>,
    state: StateBody,
}

impl From<&SeriesState> for SeriesBody {
    fn from(series: &SeriesState) -> Self {
        Self {
            labels: labelled(&series.labels),
            status: series.state.status.as_str(),
            value: series.state.value,
        }
    }
}

impl From<Watched> for RuleView {
    fn from(watched: Watched) -> Self {
        let overall = watched.overall();
        let Watched { rule, series } = watched;
        let definition = &rule.definition;
        Self {
            id: rule.id.as_uuid(),
            name: rule.name.as_str().to_owned(),
            project: rule.project.map(ProjectId::as_uuid),
            enabled: rule.enabled,
            silenced_until_ms: rule.silenced_until_ms,
            signal: SignalBody::of(definition.signal()),
            condition: ConditionBody {
                op: definition.condition().op().as_str().to_owned(),
                threshold: definition.condition().threshold(),
            },
            window_s: definition.window_secs(),
            pending_s: definition.pending_secs(),
            severity: definition.severity().as_str(),
            no_data: definition.no_data().as_str(),
            channels: definition
                .channels()
                .iter()
                .map(|channel| channel.as_uuid())
                .collect(),
            page: definition.page().map(|schedule| PageBody {
                schedule: schedule.as_uuid(),
            }),
            state: StateBody {
                status: overall.status.as_str(),
                value: overall.value,
                since_ms: overall.since_ms,
                evaluated_ms: overall.evaluated_ms,
                series: series.iter().map(SeriesBody::from).collect(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Rules {
    alerts: Vec<RuleView>,
}

#[derive(Debug, Serialize)]
struct EventBody {
    at_ms: u64,
    from: &'static str,
    to: &'static str,
    value: Option<f64>,
    labels: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<ContextBody>,
}

#[derive(Debug, Serialize)]
pub struct History {
    events: Vec<EventBody>,
}

impl From<&AlertEvent> for EventBody {
    fn from(event: &AlertEvent) -> Self {
        Self {
            at_ms: event.at_ms,
            from: event.from.as_str(),
            to: event.to.as_str(),
            value: event.value,
            labels: labelled(&event.labels),
            context: event.context.as_ref().map(ContextBody::from),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Span {
    #[serde(default)]
    from: Option<u64>,
    #[serde(default)]
    to: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct PreviewBody {
    step_ns: u64,
    points: Vec<(u64, Option<f64>)>,
    threshold: f64,
    would_fire: bool,
}

impl From<Preview> for PreviewBody {
    fn from(preview: Preview) -> Self {
        Self {
            step_ns: preview.step_nanos,
            points: preview.points,
            threshold: preview.threshold,
            would_fire: preview.would_fire,
        }
    }
}

#[derive(Debug, Serialize)]
struct DeliveryBody {
    channel: Uuid,
    status: &'static str,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Deliveries {
    deliveries: Vec<DeliveryBody>,
}

impl From<Delivery> for DeliveryBody {
    fn from(delivery: Delivery) -> Self {
        let (status, error) = match delivery.outcome {
            Ok(()) => ("sent", None),
            Err(error) => ("failed", Some(error.0.into_string())),
        };
        Self {
            channel: delivery.channel.as_uuid(),
            status,
            error,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ChannelBody {
    id: Uuid,
    name: String,
    kind: &'static str,
    target: String,
    has_secret: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    design: Option<DesignBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,
}

impl From<ChannelView> for ChannelBody {
    fn from(channel: ChannelView) -> Self {
        Self {
            id: channel.id.as_uuid(),
            name: channel.name.as_str().to_owned(),
            kind: channel.kind().as_str(),
            target: channel.destination.target(),
            has_secret: channel.has_secret,
            design: channel.destination.design().map(DesignBody::from),
            secret: None,
        }
    }
}

impl From<IssuedChannel> for ChannelBody {
    fn from(issued: IssuedChannel) -> Self {
        let secret = issued
            .secret
            .as_ref()
            .map(|secret| secret.exposed().to_owned());
        Self {
            secret,
            ..Self::from(issued.channel)
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Channels {
    channels: Vec<ChannelBody>,
}

#[derive(Debug, Deserialize)]
pub struct ChannelAsked {
    name: String,
    kind: String,
    target: String,
    #[serde(default)]
    design: Option<DesignBody>,
}

#[derive(Debug, Deserialize)]
pub struct PreviewAsked {
    kind: String,
    #[serde(default)]
    design: DesignBody,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChannelPatch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    design: Option<DesignBody>,
}

async fn list<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Rules>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let who = reading(&state, &headers).await?;
    let watched = state.app.list_alerts(&who, TenantId::new(org)).await?;
    Ok(Json(Rules {
        alerts: watched.into_iter().map(RuleView::from).collect(),
    }))
}

async fn one<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<RuleView>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let who = reading(&state, &headers).await?;
    let watched = state
        .app
        .alert(&who, TenantId::new(org), RuleId::new(id))
        .await?;
    Ok(Json(watched.into()))
}

async fn create<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<RuleBody>,
) -> Result<(StatusCode, Json<RuleView>), AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    let draft = body.draft()?;
    let watched = state
        .app
        .create_alert(&token, TenantId::new(org), &draft)
        .await?;
    tracing::info!(org = %org, alert = %watched.rule.id, "an alert rule was created");
    Ok((StatusCode::CREATED, Json(watched.into())))
}

async fn update<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(body): Json<RulePatchBody>,
) -> Result<Json<RuleView>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    let patch = body.patch()?;
    let watched = state
        .app
        .update_alert(&token, TenantId::new(org), RuleId::new(id), patch)
        .await?;
    Ok(Json(watched.into()))
}

async fn delete<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    state
        .app
        .delete_alert(&token, TenantId::new(org), RuleId::new(id))
        .await?;
    tracing::info!(org = %org, alert = %id, "an alert rule was deleted");
    Ok(StatusCode::NO_CONTENT)
}

async fn history<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    Query(span): Query<Span>,
    headers: HeaderMap,
) -> Result<Json<History>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let who = reading(&state, &headers).await?;
    let events = state
        .app
        .alert_history(
            &who,
            TenantId::new(org),
            RuleId::new(id),
            span.from,
            span.to,
        )
        .await?;
    Ok(Json(History {
        events: events.iter().map(EventBody::from).collect(),
    }))
}

async fn preview<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<RuleBody>,
) -> Result<Json<PreviewBody>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let who = reading(&state, &headers).await?;
    let (_, project, _, definition) = body.definition()?;
    let preview = state
        .app
        .preview_alert(&who, TenantId::new(org), project, &definition)
        .await?;
    Ok(Json(preview.into()))
}

async fn test<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<Deliveries>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    let deliveries = state
        .app
        .test_alert(&token, TenantId::new(org), RuleId::new(id))
        .await?;
    Ok(Json(Deliveries {
        deliveries: deliveries.into_iter().map(DeliveryBody::from).collect(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct SilenceAsked {
    until_ms: u64,
}

async fn silence<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<SilenceAsked>,
) -> Result<Json<RuleView>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    let watched = state
        .app
        .silence_alert(&token, TenantId::new(org), RuleId::new(id), asked.until_ms)
        .await?;
    Ok(Json(watched.into()))
}

async fn unsilence<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<RuleView>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    let watched = state
        .app
        .unsilence_alert(&token, TenantId::new(org), RuleId::new(id))
        .await?;
    Ok(Json(watched.into()))
}

async fn channels<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Channels>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let who = reading(&state, &headers).await?;
    let held = state.app.list_channels(&who, TenantId::new(org)).await?;
    Ok(Json(Channels {
        channels: held.into_iter().map(ChannelBody::from).collect(),
    }))
}

async fn create_channel<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<ChannelAsked>,
) -> Result<(StatusCode, Json<ChannelBody>), AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    let name = ChannelName::try_from(asked.name.as_str())?;
    let kind = ChannelKind::parse(asked.kind.trim()).ok_or(AlertRefused::Malformed("kind"))?;
    let destination = Destination::parse(kind, &asked.target)?;
    let destination = match asked.design {
        None => destination,
        Some(_) if destination.design().is_none() => return Err(BadChannel::NoDesign.into()),
        Some(design) => destination.designed(design.into_design()?),
    };
    let issued = state
        .app
        .create_channel(&token, TenantId::new(org), &name, &destination)
        .await?;
    tracing::info!(org = %org, channel = %issued.channel.id, "an alert channel was created");
    Ok((StatusCode::CREATED, Json(issued.into())))
}

async fn update_channel<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<ChannelPatch>,
) -> Result<Json<ChannelBody>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    let name = asked
        .name
        .as_deref()
        .map(ChannelName::try_from)
        .transpose()?;
    let design = asked.design.map(DesignBody::into_design).transpose()?;
    let changed = state
        .app
        .update_channel(
            &token,
            TenantId::new(org),
            ChannelId::new(id),
            ChannelChange {
                name: name.as_ref(),
                target: asked.target.as_deref(),
                design: design.as_ref(),
            },
        )
        .await?;
    Ok(Json(changed.into()))
}

async fn preview_channel<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<PreviewAsked>,
) -> Result<Json<serde_json::Value>, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    let chat = match ChannelKind::parse(asked.kind.trim()) {
        Some(ChannelKind::Slack) => Chat::Slack,
        Some(ChannelKind::Discord) => Chat::Discord,
        _ => return Err(AlertRefused::Malformed("kind")),
    };
    let to = match asked.state.as_deref() {
        None => AlertStatus::Firing,
        Some(given) => AlertStatus::parse(given.trim()).ok_or(AlertRefused::Malformed("state"))?,
    };
    let message = state
        .app
        .preview_channel_design(
            &token,
            TenantId::new(org),
            chat,
            &asked.design.into_design()?,
            to,
        )
        .await?;
    let message: serde_json::Value =
        serde_json::from_str(&message).map_err(|_| AlertRefused::Malformed("design"))?;
    Ok(Json(json!({"message": message})))
}

async fn delete_channel<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, AlertRefused>
where
    S::Repos: AlertRepos,
{
    let token = carried(&state, &headers).await?;
    state
        .app
        .delete_channel(&token, TenantId::new(org), ChannelId::new(id))
        .await?;
    tracing::info!(org = %org, channel = %id, "an alert channel was deleted");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, thiserror::Error)]
pub enum AlertRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error("the field {0} is not one this route understands")]
    Malformed(&'static str),
    #[error(transparent)]
    Rule(#[from] BadRule),
    #[error(transparent)]
    Channel(#[from] BadChannel),
    #[error(transparent)]
    Design(#[from] BadDesign),
    #[error(transparent)]
    Refused(AlertError),
}

impl From<AlertError> for AlertRefused {
    fn from(error: AlertError) -> Self {
        match error {
            AlertError::NoSession => Self::Reaching(NotCarried::NoSession),
            other => Self::Refused(other),
        }
    }
}

fn status_of(error: AlertError) -> StatusCode {
    match error {
        AlertError::NoSession => StatusCode::UNAUTHORIZED,
        AlertError::NotYours
        | AlertError::NoSuchRule
        | AlertError::NoSuchChannel
        | AlertError::NoSuchProject => StatusCode::NOT_FOUND,
        AlertError::NotYoursToChange | AlertError::OutOfScope => StatusCode::FORBIDDEN,
        AlertError::UnknownChannel
        | AlertError::UnknownSchedule
        | AlertError::NameTaken
        | AlertError::NoSecretKeeper
        | AlertError::PrivateTarget
        | AlertError::Rule(_)
        | AlertError::Channel(_)
        | AlertError::Silence(_)
        | AlertError::Query(_) => StatusCode::UNPROCESSABLE_ENTITY,
        AlertError::SignalUnavailable | AlertError::StoreUnreachable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

impl IntoResponse for AlertRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Malformed(_) | Self::Rule(_) | Self::Channel(_) | Self::Design(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
            }
            Self::Refused(AlertError::NotYours) => {
                (StatusCode::NOT_FOUND, "no such organisation".to_owned())
            }
            Self::Refused(error) => (status_of(error), error.to_string()),
        };
        if status.is_server_error() {
            tracing::error!(%message, "the alerts could not be served");
        }
        (status, message).into_response()
    }
}
