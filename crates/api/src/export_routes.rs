use std::collections::BTreeMap;
use std::io::{self, Write};
use std::mem;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferristrace_application::registry::ExportRepos;
use ferristrace_application::{Ferristrace, Stack};
use ferristrace_domain::{
    AccountId, ApiKey, EXPORT_PAGE, ExportError, ExportedSpan, Logged, Member, Prepared, Project,
    ProjectId, Recorded, RetentionDays, Snapshot, Span, SpanCursor, SpanEvent, SpanKind,
    SpanStatus, Team, TenantId, UnixSeconds,
};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Serialize;
use time::OffsetDateTime;
use tokio::sync::mpsc::Sender;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::carried::NotCarried;
use crate::session_cookie;
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;
const MILLIS_PER_SECOND: u64 = 1_000;
const FORMAT: &str = "ferristrace-export";
const VERSION: u32 = 1;
const PIPE_DEPTH: usize = 2;
const CONTROL_PLANE: &str = "control-plane.json";

type Chunk = Result<Bytes, io::Error>;

pub fn export_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: ExportRepos,
{
    Router::new()
        .route("/api/orgs/{org}/export", get(export::<S>))
        .with_state(state)
}

async fn export<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ExportRefused>
where
    S::Repos: ExportRepos,
{
    let token =
        session_cookie::carried(&headers).ok_or(ExportRefused::Reaching(NotCarried::NoSession))?;
    let prepared = state.app.begin_export(&token, TenantId::new(org)).await?;
    tracing::info!(org = %org, "an organisation is being exported");

    let filename = format!("ferristrace-{org}-{}.tar.gz", day_of(prepared.exported_at));
    let (sender, receiver) = tokio::sync::mpsc::channel(PIPE_DEPTH);
    tokio::spawn(stream(Arc::clone(&state.app), prepared, sender));

    Ok((
        [
            (CONTENT_TYPE, "application/gzip".to_owned()),
            (
                CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
            (CACHE_CONTROL, "no-store".to_owned()),
        ],
        Body::from_stream(ReceiverStream::new(receiver)),
    )
        .into_response())
}

fn day_of(at: UnixSeconds) -> String {
    let seconds = i64::try_from(at.get()).unwrap_or(i64::MAX);
    OffsetDateTime::from_unix_timestamp(seconds).map_or_else(
        |_| "00000000".to_owned(),
        |moment| {
            format!(
                "{:04}{:02}{:02}",
                moment.year(),
                u8::from(moment.month()),
                moment.day()
            )
        },
    )
}

#[derive(Debug, thiserror::Error)]
enum Interrupted {
    #[error("the archive could not be written: {0}")]
    Archive(#[from] io::Error),
    #[error("a record could not be rendered: {0}")]
    Render(#[from] serde_json::Error),
    #[error("the spans could not be read: {0}")]
    Spans(#[from] ExportError),
    #[error("the download was abandoned")]
    Abandoned,
}

async fn stream<S: Stack>(app: Arc<Ferristrace<S>>, prepared: Prepared, sender: Sender<Chunk>) {
    match write_archive(&app, &prepared, &sender).await {
        Ok(()) => {}
        Err(Interrupted::Abandoned) => tracing::info!("an export was abandoned by its reader"),
        Err(error) => {
            tracing::error!(%error, "an export stopped before its end");
            let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
        }
    }
}

async fn write_archive<S: Stack>(
    app: &Ferristrace<S>,
    prepared: &Prepared,
    sender: &Sender<Chunk>,
) -> Result<(), Interrupted> {
    let mut archive = Archive::new(prepared.exported_at.get());
    let control_plane =
        serde_json::to_vec_pretty(&ControlPlane::of(&prepared.snapshot, prepared.exported_at))?;
    archive.entry(CONTROL_PLANE, &control_plane)?;
    hand_over(sender, archive.drain()).await?;

    let mut after: Option<SpanCursor> = None;
    let mut number = 0_u32;
    loop {
        let page = app
            .export_span_page(prepared.grant, after.as_ref(), EXPORT_PAGE)
            .await?;
        if !page.spans.is_empty() {
            number += 1;
            let mut lines = Vec::new();
            for row in &page.spans {
                serde_json::to_writer(&mut lines, &SpanLine::of(row))?;
                lines.push(b'\n');
            }
            archive.entry(&format!("spans/{number:06}.ndjson"), &lines)?;
            hand_over(sender, archive.drain()).await?;
        }
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    hand_over(sender, archive.finish()?).await
}

async fn hand_over(sender: &Sender<Chunk>, bytes: Vec<u8>) -> Result<(), Interrupted> {
    if bytes.is_empty() {
        return Ok(());
    }
    sender
        .send(Ok(Bytes::from(bytes)))
        .await
        .map_err(|_| Interrupted::Abandoned)
}

struct Archive {
    builder: tar::Builder<GzEncoder<Vec<u8>>>,
    modified: u64,
}

impl Archive {
    fn new(modified: u64) -> Self {
        Self {
            builder: tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default())),
            modified,
        }
    }

    fn entry(&mut self, path: &str, data: &[u8]) -> io::Result<()> {
        let mut header = tar::Header::new_gnu();
        header.set_size(u64::try_from(data.len()).unwrap_or(u64::MAX));
        header.set_mode(0o644);
        header.set_mtime(self.modified);
        header.set_entry_type(tar::EntryType::Regular);
        self.builder.append_data(&mut header, path, data)?;
        self.builder.get_mut().flush()
    }

    fn drain(&mut self) -> Vec<u8> {
        mem::take(self.builder.get_mut().get_mut())
    }

    fn finish(self) -> io::Result<Vec<u8>> {
        self.builder.into_inner()?.finish()
    }
}

#[derive(Debug, Serialize)]
struct ControlPlane<'a> {
    format: &'static str,
    version: u32,
    exported_at_ms: u64,
    org: OrgLine<'a>,
    projects: Vec<ProjectLine<'a>>,
    teams: Vec<TeamLine<'a>>,
    members: Vec<MemberLine<'a>>,
    keys: Vec<KeyLine<'a>>,
    issue_decisions: Vec<DecisionLine<'a>>,
    audit_entries: Vec<AuditLine<'a>>,
}

impl<'a> ControlPlane<'a> {
    fn of(snapshot: &'a Snapshot, at: UnixSeconds) -> Self {
        Self {
            format: FORMAT,
            version: VERSION,
            exported_at_ms: at.get().saturating_mul(MILLIS_PER_SECOND),
            org: OrgLine {
                id: snapshot.org.id.as_uuid(),
                name: &snapshot.org.name,
                plan: &snapshot.org.plan,
                created_at_ms: millis(snapshot.org.created_at_unix_nanos),
                retention_days: snapshot.org.retention.map(RetentionDays::get),
            },
            projects: snapshot.projects.iter().map(ProjectLine::of).collect(),
            teams: snapshot.teams.iter().map(TeamLine::of).collect(),
            members: snapshot.members.iter().map(MemberLine::of).collect(),
            keys: snapshot.keys.iter().map(KeyLine::of).collect(),
            issue_decisions: snapshot.decisions.iter().map(DecisionLine::of).collect(),
            audit_entries: snapshot.audit.iter().map(AuditLine::of).collect(),
        }
    }
}

fn millis(nanos: u64) -> u64 {
    nanos / NANOS_PER_MILLI
}

#[derive(Debug, Serialize)]
struct OrgLine<'a> {
    id: Uuid,
    name: &'a str,
    plan: &'a str,
    created_at_ms: u64,
    retention_days: Option<u32>,
}

#[derive(Debug, Serialize)]
struct Named<'a> {
    id: Uuid,
    name: &'a str,
}

#[derive(Debug, Serialize)]
struct ProjectLine<'a> {
    id: Uuid,
    name: &'a str,
    created_at_ms: u64,
    team: Option<Named<'a>>,
}

impl<'a> ProjectLine<'a> {
    fn of(project: &'a Project) -> Self {
        Self {
            id: project.id.as_uuid(),
            name: &project.name,
            created_at_ms: millis(project.created_at_unix_nanos),
            team: project.team.as_ref().map(|team| Named {
                id: team.id.as_uuid(),
                name: &team.name,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
struct TeamLine<'a> {
    id: Uuid,
    name: &'a str,
    created_at_ms: u64,
    members: Vec<Uuid>,
    projects: Vec<Named<'a>>,
}

impl<'a> TeamLine<'a> {
    fn of(team: &'a Team) -> Self {
        Self {
            id: team.id.as_uuid(),
            name: &team.name,
            created_at_ms: millis(team.created_at_unix_nanos),
            members: team.members.iter().map(|member| member.as_uuid()).collect(),
            projects: team
                .projects
                .iter()
                .map(|project| Named {
                    id: project.id.as_uuid(),
                    name: &project.name,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct MemberLine<'a> {
    account_id: Uuid,
    email: &'a str,
    name: Option<&'a str>,
    role: &'static str,
    joined_at_ms: u64,
}

impl<'a> MemberLine<'a> {
    fn of(member: &'a Member) -> Self {
        Self {
            account_id: member.account.as_uuid(),
            email: member.email.as_str(),
            name: member.name.as_deref(),
            role: member.role.as_str(),
            joined_at_ms: millis(member.joined_at_unix_nanos),
        }
    }
}

#[derive(Debug, Serialize)]
struct KeyLine<'a> {
    id: Uuid,
    name: &'a str,
    prefix: &'a str,
    project: Option<Uuid>,
    scopes: Vec<&'static str>,
    created_at_ms: u64,
    created_by: Option<Uuid>,
    last_used_at_ms: Option<u64>,
}

impl<'a> KeyLine<'a> {
    fn of(key: &'a ApiKey) -> Self {
        Self {
            id: key.id.as_uuid(),
            name: &key.name,
            prefix: &key.prefix,
            project: key.project.map(ProjectId::as_uuid),
            scopes: key
                .scopes
                .as_slice()
                .iter()
                .map(|scope| scope.as_str())
                .collect(),
            created_at_ms: millis(key.created_at_unix_nanos),
            created_by: key.created_by.map(AccountId::as_uuid),
            last_used_at_ms: key.last_used_at_unix_nanos.map(millis),
        }
    }
}

#[derive(Debug, Serialize)]
struct DecisionLine<'a> {
    project_id: Uuid,
    fingerprint: String,
    status: &'a str,
    owner_id: Option<Uuid>,
    decided_at_ms: u64,
    decided_by: Option<Uuid>,
}

impl DecisionLine<'_> {
    fn of(recorded: &Recorded) -> Self {
        Self {
            project_id: recorded.project.as_uuid(),
            fingerprint: hex::encode(recorded.print.as_bytes()),
            status: recorded.decision.status.as_str(),
            owner_id: recorded.decision.owner.map(AccountId::as_uuid),
            decided_at_ms: millis(recorded.decision.decided_at_unix_nanos),
            decided_by: recorded.decision.decided_by.map(AccountId::as_uuid),
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditLine<'a> {
    id: Uuid,
    at_ms: u64,
    actor: Who<'a>,
    action: &'static str,
    target: What<'a>,
    detail: BTreeMap<&'a str, Option<&'a str>>,
}

#[derive(Debug, Serialize)]
struct Who<'a> {
    kind: &'static str,
    id: Option<Uuid>,
    label: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct What<'a> {
    kind: &'static str,
    id: Option<&'a str>,
}

impl<'a> AuditLine<'a> {
    fn of(logged: &'a Logged) -> Self {
        Self {
            id: logged.id.as_uuid(),
            at_ms: millis(logged.at_unix_nanos),
            actor: Who {
                kind: logged.entry.actor.kind(),
                id: logged.entry.actor.id(),
                label: logged.label.as_deref(),
            },
            action: logged.entry.action.as_str(),
            target: What {
                kind: logged.entry.target.kind.as_str(),
                id: logged.entry.target.id.as_deref(),
            },
            detail: logged
                .entry
                .detail
                .pairs()
                .iter()
                .map(|(key, value)| (&**key, value.as_deref()))
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct SpanLine<'a> {
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    project_id: Option<Uuid>,
    service_name: &'a str,
    name: &'a str,
    kind: &'static str,
    status: &'static str,
    start_unix_nanos: u64,
    end_unix_nanos: u64,
    attributes: &'a BTreeMap<Box<str>, Box<str>>,
    events: Vec<EventLine<'a>>,
}

#[derive(Debug, Serialize)]
struct EventLine<'a> {
    time_unix_nanos: u64,
    name: &'a str,
    attributes: &'a BTreeMap<Box<str>, Box<str>>,
}

impl<'a> SpanLine<'a> {
    fn of(row: &'a ExportedSpan) -> Self {
        let span: &Span = &row.span;
        Self {
            trace_id: hex::encode(span.trace_id.as_bytes()),
            span_id: hex::encode(span.span_id.as_bytes()),
            parent_span_id: span.parent.map(|parent| hex::encode(parent.as_bytes())),
            project_id: row.project.map(ProjectId::as_uuid),
            service_name: &span.service_name,
            name: &span.name,
            kind: kind_of(span.kind),
            status: status_of(span.status),
            start_unix_nanos: span.start_unix_nanos,
            end_unix_nanos: span.end_unix_nanos,
            attributes: &span.attributes,
            events: span.events.iter().map(EventLine::of).collect(),
        }
    }
}

impl<'a> EventLine<'a> {
    fn of(event: &'a SpanEvent) -> Self {
        Self {
            time_unix_nanos: event.time_unix_nanos,
            name: &event.name,
            attributes: &event.attributes,
        }
    }
}

fn kind_of(kind: SpanKind) -> &'static str {
    match kind {
        SpanKind::Unspecified => "unspecified",
        SpanKind::Internal => "internal",
        SpanKind::Server => "server",
        SpanKind::Client => "client",
        SpanKind::Producer => "producer",
        SpanKind::Consumer => "consumer",
    }
}

fn status_of(status: SpanStatus) -> &'static str {
    match status {
        SpanStatus::Unset => "unset",
        SpanStatus::Ok => "ok",
        SpanStatus::Error => "error",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("exporting an organisation is an owner's to do")]
    NotAnOwner,
    #[error("the organisation could not be exported")]
    Unavailable,
}

impl From<ExportError> for ExportRefused {
    fn from(error: ExportError) -> Self {
        match error {
            ExportError::NoSession => Self::Reaching(NotCarried::NoSession),
            ExportError::NotYours => Self::NoSuchOrg,
            ExportError::OwnersOnly => Self::NotAnOwner,
            ExportError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for ExportRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::NoSuchOrg => StatusCode::NOT_FOUND,
            Self::NotAnOwner => StatusCode::FORBIDDEN,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "an export could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
