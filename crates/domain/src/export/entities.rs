use crate::access::ApiKey;
use crate::audit::Logged;
use crate::catalogue::{Project, ProjectId};
use crate::clock::UnixSeconds;
use crate::issue::Recorded;
use crate::organisation::{Member, Organisation, Team};
use crate::telemetry::{Span, SpanId, TenantId, TraceId};

pub const EXPORT_PAGE: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub org: Organisation,
    pub projects: Vec<Project>,
    pub teams: Vec<Team>,
    pub members: Vec<Member>,
    pub keys: Vec<ApiKey>,
    pub decisions: Vec<Recorded>,
    pub audit: Vec<Logged>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportGrant {
    org: TenantId,
}

impl ExportGrant {
    pub(super) fn of(org: TenantId) -> Self {
        Self { org }
    }

    pub fn org(self) -> TenantId {
        self.org
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub grant: ExportGrant,
    pub exported_at: UnixSeconds,
    pub snapshot: Snapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanCursor {
    service_name: Box<str>,
    start_unix_nanos: u64,
    trace_id: TraceId,
    span_id: SpanId,
}

impl SpanCursor {
    pub fn after(span: &Span) -> Self {
        Self {
            service_name: span.service_name.clone(),
            start_unix_nanos: span.start_unix_nanos,
            trace_id: span.trace_id,
            span_id: span.span_id,
        }
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub fn start_unix_nanos(&self) -> u64 {
        self.start_unix_nanos
    }

    pub fn trace_id(&self) -> TraceId {
        self.trace_id
    }

    pub fn span_id(&self) -> SpanId {
        self.span_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedSpan {
    pub project: Option<ProjectId>,
    pub span: Span,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanPage {
    pub spans: Vec<ExportedSpan>,
    pub next: Option<SpanCursor>,
}

impl SpanPage {
    pub fn cut(mut probed: Vec<ExportedSpan>, limit: u32) -> Self {
        let kept = usize::try_from(limit).unwrap_or(usize::MAX);
        if probed.len() <= kept {
            return Self {
                spans: probed,
                next: None,
            };
        }
        probed.truncate(kept);
        let next = probed.last().map(|last| SpanCursor::after(&last.span));
        Self {
            spans: probed,
            next,
        }
    }
}
