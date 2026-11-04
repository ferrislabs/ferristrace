use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use ferristrace_application::Stack;
use ferristrace_application::registry::LlmRepos;
use ferristrace_domain::{
    BadCallQuery, BadPrice, BadSpendQuery, CallCursor, CallPage, CallQuery, GroupBy, LlmError,
    ModelPattern, Price, PriceDraft, PriceId, PricedCall, ProjectId, PromptMarker, Provider, Rates,
    Spend, SpendGroup, SpendQuery, TenantId, TraceId, Unpriced, Window,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried, reading};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

pub fn llm_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: LlmRepos,
{
    Router::new()
        .route("/api/orgs/{org}/llm/calls", get(calls::<S>))
        .route("/api/orgs/{org}/llm/spend", get(spend::<S>))
        .route("/api/orgs/{org}/llm/unpriced", get(unpriced::<S>))
        .route(
            "/api/orgs/{org}/llm/prices",
            get(prices::<S>).post(add_price::<S>),
        )
        .route("/api/orgs/{org}/llm/prices/{id}", delete(remove_price::<S>))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct AskedCalls {
    from: u64,
    to: u64,
    #[serde(default)]
    feature: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    customer: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    trace: Option<String>,
    #[serde(default)]
    project: Option<Uuid>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    before: Option<String>,
}

fn present(text: Option<String>) -> Option<Box<str>> {
    text.map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .map(Into::into)
}

fn window_of(from: u64, to: u64) -> Result<Window, LlmRefused> {
    Window::new(
        from.saturating_mul(NANOS_PER_MILLI),
        to.saturating_mul(NANOS_PER_MILLI),
    )
    .map_err(|_| LlmRefused::Window)
}

fn cursor_of(spelling: &str) -> Result<CallCursor, LlmRefused> {
    let (time, tiebreak) = spelling
        .split_once('.')
        .ok_or(LlmRefused::Malformed("before"))?;
    Ok(CallCursor {
        time_unix_nanos: time.parse().map_err(|_| LlmRefused::Malformed("before"))?,
        tiebreak: tiebreak
            .parse()
            .map_err(|_| LlmRefused::Malformed("before"))?,
    })
}

fn spelled(cursor: CallCursor) -> String {
    format!("{}.{}", cursor.time_unix_nanos, cursor.tiebreak)
}

impl AskedCalls {
    fn query(self) -> Result<CallQuery, LlmRefused> {
        let mut query = CallQuery::over(window_of(self.from, self.to)?);
        if let Some(limit) = self.limit {
            query = query.at_most(limit)?;
        }
        query.feature = present(self.feature);
        query.model = present(self.model);
        query.customer = present(self.customer);
        query.user = present(self.user);
        query.prompt_version = present(self.prompt);
        query.provider = present(self.provider);
        query.environment = present(self.environment);
        query.project = self.project.map(ProjectId::new);
        query.trace = present(self.trace)
            .map(|spelling| {
                hex::decode(&*spelling)
                    .ok()
                    .and_then(|bytes| TraceId::try_from(bytes.as_slice()).ok())
                    .ok_or(LlmRefused::Malformed("trace"))
            })
            .transpose()?;
        query.before = present(self.before)
            .map(|spelling| cursor_of(&spelling))
            .transpose()?;
        Ok(query)
    }
}

#[derive(Debug, Deserialize)]
pub struct AskedSpend {
    from: u64,
    to: u64,
    #[serde(default)]
    group_by: Option<String>,
    #[serde(default)]
    feature: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    customer: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    project: Option<Uuid>,
}

impl AskedSpend {
    fn query(self) -> Result<SpendQuery, LlmRefused> {
        let mut query = SpendQuery::over(window_of(self.from, self.to)?);
        if let Some(group_by) = present(self.group_by) {
            query.group_by = GroupBy::try_from(&*group_by)?;
        }
        query.filter.feature = present(self.feature);
        query.filter.model = present(self.model);
        query.filter.customer = present(self.customer);
        query.filter.user = present(self.user);
        query.filter.prompt_version = present(self.prompt);
        query.filter.project = self.project.map(ProjectId::new);
        Ok(query)
    }
}

#[derive(Debug, Serialize)]
pub struct Spent {
    step_ms: u64,
    groups: Vec<SpentGroup>,
    markers: Vec<Marked>,
}

#[derive(Debug, Serialize)]
struct SpentGroup {
    key: String,
    cost: f64,
    unpriced: u64,
    calls: u64,
    input_tokens: u64,
    output_tokens: u64,
    p50_ms: f64,
    p95_ms: f64,
    errors: u64,
    points: Vec<(u64, f64)>,
}

#[derive(Debug, Serialize)]
struct Marked {
    time_ms: u64,
    feature: String,
    prompt_version: String,
}

impl From<SpendGroup> for SpentGroup {
    fn from(group: SpendGroup) -> Self {
        Self {
            key: group.key.into_string(),
            cost: group.cost.dollars(),
            unpriced: group.unpriced,
            calls: group.calls,
            input_tokens: group.input_tokens,
            output_tokens: group.output_tokens,
            p50_ms: group.p50_nanos as f64 / NANOS_PER_MILLI as f64,
            p95_ms: group.p95_nanos as f64 / NANOS_PER_MILLI as f64,
            errors: group.errors,
            points: group
                .points
                .into_iter()
                .map(|(start, cost)| (start / NANOS_PER_MILLI, cost.dollars()))
                .collect(),
        }
    }
}

impl From<PromptMarker> for Marked {
    fn from(marker: PromptMarker) -> Self {
        Self {
            time_ms: marker.time_unix_nanos / NANOS_PER_MILLI,
            feature: marker.feature.into_string(),
            prompt_version: marker.prompt_version.into_string(),
        }
    }
}

impl From<Spend> for Spent {
    fn from(spend: Spend) -> Self {
        Self {
            step_ms: (spend.step_nanos / NANOS_PER_MILLI).max(1),
            groups: spend.groups.into_iter().map(Into::into).collect(),
            markers: spend.markers.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Calls {
    calls: Vec<Shown>,
    next: Option<String>,
}

#[derive(Debug, Serialize)]
struct Shown {
    time_ms: u64,
    trace_id: String,
    span_id: String,
    service: String,
    provider: String,
    operation: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cost: Option<f64>,
    duration_ms: f64,
    error: bool,
    feature: String,
    customer: String,
    user: String,
    prompt_version: String,
    environment: String,
}

impl From<PricedCall> for Shown {
    fn from(priced: PricedCall) -> Self {
        let call = priced.call;
        let model = call.model().to_owned();
        Self {
            time_ms: call.time_ms(),
            trace_id: hex::encode(call.trace.as_bytes()),
            span_id: hex::encode(call.span.as_bytes()),
            service: call.service.into_string(),
            provider: call.provider.into_string(),
            operation: call.operation.into_string(),
            model,
            input_tokens: call.input_tokens,
            output_tokens: call.output_tokens,
            cached_input_tokens: call.cached_input_tokens,
            cost: priced.cost.map(ferristrace_domain::Cost::dollars),
            duration_ms: call.duration_nanos as f64 / NANOS_PER_MILLI as f64,
            error: call.error,
            feature: call.feature.into_string(),
            customer: call.customer.into_string(),
            user: call.user.into_string(),
            prompt_version: call.prompt_version.into_string(),
            environment: call.environment.into_string(),
        }
    }
}

impl From<CallPage> for Calls {
    fn from(page: CallPage) -> Self {
        Self {
            calls: page.calls.into_iter().map(Into::into).collect(),
            next: page.next.map(spelled),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AskedWindow {
    from: u64,
    to: u64,
}

#[derive(Debug, Serialize)]
pub struct Unpriceds {
    models: Vec<Missing>,
}

#[derive(Debug, Serialize)]
struct Missing {
    provider: String,
    model: String,
    calls: u64,
    last_seen_ms: u64,
}

impl From<Unpriced> for Missing {
    fn from(unpriced: Unpriced) -> Self {
        Self {
            provider: unpriced.provider.into_string(),
            model: unpriced.model.into_string(),
            calls: unpriced.calls,
            last_seen_ms: unpriced.last_unix_nanos / NANOS_PER_MILLI,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Prices {
    prices: Vec<PriceBody>,
}

#[derive(Debug, Serialize)]
pub struct PriceBody {
    id: Uuid,
    provider: String,
    model: String,
    input: u64,
    output: u64,
    cached_input: Option<u64>,
    effective_from_ms: u64,
}

impl From<Price> for PriceBody {
    fn from(price: Price) -> Self {
        Self {
            id: price.id.as_uuid(),
            provider: price.provider.to_string(),
            model: price.model.to_string(),
            input: price.rates.input,
            output: price.rates.output,
            cached_input: price.rates.cached_input,
            effective_from_ms: price.effective_from_ms,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PriceAsked {
    provider: String,
    model: String,
    input: u64,
    output: u64,
    #[serde(default)]
    cached_input: Option<u64>,
    #[serde(default)]
    effective_from_ms: u64,
}

impl PriceAsked {
    fn draft(&self) -> Result<PriceDraft, BadPrice> {
        Ok(PriceDraft {
            provider: Provider::try_from(self.provider.as_str())?,
            model: ModelPattern::try_from(self.model.as_str())?,
            rates: Rates::new(self.input, self.output, self.cached_input)?,
            effective_from_ms: self.effective_from_ms,
        })
    }
}

async fn calls<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<AskedCalls>,
    headers: HeaderMap,
) -> Result<Json<Calls>, LlmRefused>
where
    S::Repos: LlmRepos,
{
    let who = reading(&state, &headers).await?;
    let query = asked.query()?;
    let page = state
        .app
        .llm_calls(&who, TenantId::new(org), &query)
        .await?;
    Ok(Json(page.into()))
}

async fn spend<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<AskedSpend>,
    headers: HeaderMap,
) -> Result<Json<Spent>, LlmRefused>
where
    S::Repos: LlmRepos,
{
    let who = reading(&state, &headers).await?;
    let query = asked.query()?;
    let spent = state
        .app
        .llm_spend(&who, TenantId::new(org), &query)
        .await?;
    Ok(Json(spent.into()))
}

async fn unpriced<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<AskedWindow>,
    headers: HeaderMap,
) -> Result<Json<Unpriceds>, LlmRefused>
where
    S::Repos: LlmRepos,
{
    let who = reading(&state, &headers).await?;
    let window = window_of(asked.from, asked.to)?;
    let found = state
        .app
        .llm_unpriced(&who, TenantId::new(org), window)
        .await?;
    Ok(Json(Unpriceds {
        models: found.into_iter().map(Into::into).collect(),
    }))
}

async fn prices<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Prices>, LlmRefused>
where
    S::Repos: LlmRepos,
{
    let who = reading(&state, &headers).await?;
    let held = state.app.llm_prices(&who, TenantId::new(org)).await?;
    Ok(Json(Prices {
        prices: held.into_iter().map(Into::into).collect(),
    }))
}

async fn add_price<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<PriceAsked>,
) -> Result<(StatusCode, Json<PriceBody>), LlmRefused>
where
    S::Repos: LlmRepos,
{
    let token = carried(&state, &headers).await?;
    let draft = asked.draft()?;
    let price = state
        .app
        .add_llm_price(&token, TenantId::new(org), &draft)
        .await?;
    tracing::info!(org = %org, price = %price.id, "a model price was set");
    Ok((StatusCode::CREATED, Json(price.into())))
}

async fn remove_price<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, LlmRefused>
where
    S::Repos: LlmRepos,
{
    let token = carried(&state, &headers).await?;
    state
        .app
        .remove_llm_price(&token, TenantId::new(org), PriceId::new(id))
        .await?;
    tracing::info!(org = %org, price = %id, "a model price was deleted");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, thiserror::Error)]
pub enum LlmRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error("a window must end after it starts")]
    Window,
    #[error("the field {0} is not one this route understands")]
    Malformed(&'static str),
    #[error(transparent)]
    Price(#[from] BadPrice),
    #[error(transparent)]
    Query(#[from] BadCallQuery),
    #[error(transparent)]
    Spend(#[from] BadSpendQuery),
    #[error(transparent)]
    Refused(LlmError),
}

impl From<LlmError> for LlmRefused {
    fn from(error: LlmError) -> Self {
        match error {
            LlmError::NoSession => Self::Reaching(NotCarried::NoSession),
            other => Self::Refused(other),
        }
    }
}

impl IntoResponse for LlmRefused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Window | Self::Query(_) | Self::Spend(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            Self::Malformed(_) | Self::Price(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
            }
            Self::Refused(LlmError::NotYours) => {
                (StatusCode::NOT_FOUND, "no such organisation".to_owned())
            }
            Self::Refused(error) => (status_of(error), error.to_string()),
        };
        if status.is_server_error() {
            tracing::error!(%message, "the llm calls could not be served");
        }
        (status, message).into_response()
    }
}

fn status_of(error: LlmError) -> StatusCode {
    match error {
        LlmError::NoSession => StatusCode::UNAUTHORIZED,
        LlmError::NotYours | LlmError::NoSuchPrice => StatusCode::NOT_FOUND,
        LlmError::NotYoursToChange | LlmError::OutOfScope => StatusCode::FORBIDDEN,
        LlmError::PriceTaken => StatusCode::CONFLICT,
        LlmError::StoreUnreachable => StatusCode::SERVICE_UNAVAILABLE,
    }
}
