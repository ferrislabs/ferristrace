use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::AuditRepos;
use ferristrace_domain::{
    Action, Actor, AuditError, AuditFilter, AuditPage, Cursor, Logged, PageSize, TenantId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Serialize)]
pub struct Trail {
    entries: Vec<Line>,
    next: Option<String>,
}

#[derive(Debug, Serialize)]
struct Line {
    id: Uuid,
    at_ms: u64,
    actor: Who,
    action: &'static str,
    target: What,
    detail: BTreeMap<Box<str>, Option<Box<str>>>,
}

#[derive(Debug, Serialize)]
struct Who {
    kind: &'static str,
    id: Option<Uuid>,
    label: Option<Box<str>>,
}

#[derive(Debug, Serialize)]
struct What {
    kind: &'static str,
    id: Option<Box<str>>,
}

impl From<Logged> for Line {
    fn from(logged: Logged) -> Self {
        let Logged {
            id,
            at_unix_nanos,
            entry,
            label,
        } = logged;
        Self {
            id: id.as_uuid(),
            at_ms: at_unix_nanos / NANOS_PER_MILLI,
            actor: Who {
                kind: entry.actor.kind(),
                id: entry.actor.id(),
                label,
            },
            action: entry.action.as_str(),
            target: What {
                kind: entry.target.kind.as_str(),
                id: entry.target.id,
            },
            detail: entry.detail.pairs().iter().cloned().collect(),
        }
    }
}

impl From<AuditPage> for Trail {
    fn from(page: AuditPage) -> Self {
        Self {
            next: page.next.as_ref().map(Cursor::token),
            entries: page.entries.into_iter().map(Line::from).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Asked {
    #[serde(default)]
    actor_kind: Option<String>,
    #[serde(default)]
    actor_id: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    before: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

fn given(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

impl Asked {
    fn filter(&self) -> Result<AuditFilter, AuditRefused> {
        let actor_id = given(self.actor_id.as_deref())
            .map(|id| Uuid::try_parse(id).map_err(|_| AuditRefused::BadActor))
            .transpose()?;
        let actor = match given(self.actor_kind.as_deref()) {
            Some(kind) => Some(Actor::of(kind, actor_id).ok_or(AuditRefused::BadActor)?),
            None if actor_id.is_some() => return Err(AuditRefused::BadActor),
            None => None,
        };
        let action = given(self.action.as_deref())
            .map(|action| Action::parse(action).ok_or(AuditRefused::BadAction))
            .transpose()?;
        let before = given(self.before.as_deref())
            .map(|before| Cursor::parse(before).ok_or(AuditRefused::BadCursor))
            .transpose()?;
        Ok(AuditFilter {
            actor,
            action,
            before,
        })
    }
}

pub fn audit_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: AuditRepos,
{
    Router::new()
        .route("/api/orgs/{org}/audit", get(read::<S>))
        .with_state(state)
}

async fn read<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    Query(asked): Query<Asked>,
    headers: HeaderMap,
) -> Result<Json<Trail>, AuditRefused>
where
    S::Repos: AuditRepos,
{
    let token = carried(&headers)?;
    let filter = asked.filter()?;
    let page = state
        .app
        .audit_log(
            &token,
            TenantId::new(org),
            &filter,
            PageSize::new(asked.limit),
        )
        .await?;
    Ok(Json(page.into()))
}

#[derive(Debug, thiserror::Error)]
pub enum AuditRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error("an actor is a kind of account, api_key or operator, and an id unless an operator")]
    BadActor,
    #[error("no audited action carries that name")]
    BadAction,
    #[error("that cursor was not handed out by this log")]
    BadCursor,
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("the audit log is an administrator's to read")]
    NotAnAdministrator,
    #[error("the audit log could not be read")]
    Unavailable,
}

impl From<AuditError> for AuditRefused {
    fn from(error: AuditError) -> Self {
        match error {
            AuditError::NoSession => Self::Reaching(NotCarried::NoSession),
            AuditError::NotYours => Self::NoSuchOrg,
            AuditError::NotYoursToRead => Self::NotAnAdministrator,
            AuditError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for AuditRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::BadActor | Self::BadAction | Self::BadCursor => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NoSuchOrg => StatusCode::NOT_FOUND,
            Self::NotAnAdministrator => StatusCode::FORBIDDEN,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the audit log could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
