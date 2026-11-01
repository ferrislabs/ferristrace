use core::fmt;
use core::future::Future;

use sha2::{Digest, Sha256};

use crate::catalogue::ProjectId;
use crate::clock::Clock;
use crate::explore::{ReadError, Window};
use crate::identity::AccountId;
use crate::membership::{Memberships, OrgRole, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::session::SessionStore;
use crate::span::{SpanId, TraceId};
use crate::tenant::TenantId;

pub const EXCEPTION_EVENT: &str = "exception";
pub const PRINT_BYTES: usize = 16;

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 500;
const HEXISH_FLOOR: usize = 8;
const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a fingerprint is 16 bytes, written as 32 hex characters")]
pub struct BadFingerprint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; PRINT_BYTES]);

impl Fingerprint {
    pub fn new(bytes: [u8; PRINT_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; PRINT_BYTES] {
        &self.0
    }

    pub fn of(service: &str, exception_type: &str, message: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(service.as_bytes());
        hasher.update(b"\0");
        hasher.update(exception_type.as_bytes());
        hasher.update(b"\0");
        hasher.update(normalise(message).as_bytes());
        let whole: [u8; 32] = hasher.finalize().into();
        let mut print = [0; PRINT_BYTES];
        print.copy_from_slice(&whole[..PRINT_BYTES]);
        Self(print)
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl TryFrom<&str> for Fingerprint {
    type Error = BadFingerprint;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        let bytes = hex::decode(text).map_err(|_| BadFingerprint)?;
        Ok(Self(bytes.try_into().map_err(|_| BadFingerprint)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub service: Box<str>,
    pub exception_type: Box<str>,
    pub message: Box<str>,
    pub stacktrace: Option<Box<str>>,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub time_unix_nanos: u64,
}

pub fn fingerprint(of: &Occurrence) -> Fingerprint {
    Fingerprint::of(&of.service, &of.exception_type, &of.message)
}

pub fn normalise(message: &str) -> String {
    unquoted(message)
        .split_whitespace()
        .map(|token| {
            let tail = token.rsplit('/').next().unwrap_or(token);
            if hexish(tail) {
                return "x".to_owned();
            }
            undigited(tail)
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn unquoted(message: &str) -> String {
    let mut kept = String::with_capacity(message.len());
    let mut open: Option<char> = None;
    for letter in message.chars() {
        match open {
            Some(quote) => {
                if letter == quote {
                    open = None;
                }
            }
            None => {
                if letter == '\'' || letter == '"' {
                    open = Some(letter);
                    kept.push_str("''");
                } else {
                    kept.push(letter);
                }
            }
        }
    }
    kept
}

fn hexish(token: &str) -> bool {
    let bare: String = token.chars().filter(|letter| *letter != '-').collect();
    bare.len() > HEXISH_FLOOR && bare.chars().all(|letter| letter.is_ascii_hexdigit())
}

fn undigited(token: &str) -> String {
    let mut kept = String::with_capacity(token.len());
    let mut running = false;
    for letter in token.chars() {
        if letter.is_ascii_digit() {
            if !running {
                kept.push('0');
                running = true;
            }
        } else {
            running = false;
            kept.push(letter);
        }
    }
    kept
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Unresolved,
    Resolved,
    Ignored,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::Resolved => "resolved",
            Self::Ignored => "ignored",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "unresolved" => Some(Self::Unresolved),
            "resolved" => Some(Self::Resolved),
            "ignored" => Some(Self::Ignored),
            _ => None,
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Decision {
    pub status: Status,
    pub owner: Option<AccountId>,
    pub decided_at_unix_nanos: u64,
    pub decided_by: Option<AccountId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted {
    pub project: ProjectId,
    pub print: Fingerprint,
    pub exception_type: Box<str>,
    pub message: Box<str>,
    pub service: Box<str>,
    pub count: u64,
    pub first_seen_unix_nanos: u64,
    pub last_seen_unix_nanos: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    pub project: ProjectId,
    pub print: Fingerprint,
    pub decision: Decision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub counted: Counted,
    pub decision: Decision,
    pub regressed: bool,
}

impl Issue {
    pub fn of(counted: Counted, decision: Decision) -> Self {
        let regressed = decision.status == Status::Resolved
            && counted.last_seen_unix_nanos > decision.decided_at_unix_nanos;
        Self {
            counted,
            decision,
            regressed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueFilter {
    pub window: Window,
    pub project: Option<ProjectId>,
    pub print: Option<Fingerprint>,
    pub service: Option<Box<str>>,
    pub text: Option<Box<str>>,
    limit: u32,
}

impl IssueFilter {
    pub fn over(window: Window) -> Self {
        Self {
            window,
            project: None,
            print: None,
            service: None,
            text: None,
            limit: DEFAULT_LIMIT,
        }
    }

    #[must_use]
    pub fn at_most(mut self, asked: u32) -> Self {
        self.limit = asked.clamp(1, MAX_LIMIT);
        self
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }
}

pub trait IssueReader {
    fn issues(
        &self,
        tenant: TenantId,
        filter: &IssueFilter,
    ) -> impl Future<Output = Result<Vec<Counted>, ReadError>> + Send;

    fn occurrences(
        &self,
        tenant: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Occurrence>, ReadError>> + Send;
}

impl<T: IssueReader + Sync> IssueReader for &T {
    fn issues(
        &self,
        tenant: TenantId,
        filter: &IssueFilter,
    ) -> impl Future<Output = Result<Vec<Counted>, ReadError>> + Send {
        (*self).issues(tenant, filter)
    }

    fn occurrences(
        &self,
        tenant: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Occurrence>, ReadError>> + Send {
        (*self).occurrences(tenant, project, print, window, limit)
    }
}

pub trait IssueDecisions {
    fn read_many(
        &self,
        of: &[(ProjectId, Fingerprint)],
    ) -> impl Future<Output = Result<Vec<Recorded>, ReadError>> + Send;

    fn decide(
        &self,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;

    fn assign(
        &self,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;
}

impl<T: IssueDecisions + Sync> IssueDecisions for &T {
    fn read_many(
        &self,
        of: &[(ProjectId, Fingerprint)],
    ) -> impl Future<Output = Result<Vec<Recorded>, ReadError>> + Send {
        (*self).read_many(of)
    }

    fn decide(
        &self,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).decide(project, print, status, by, at_unix_nanos)
    }

    fn assign(
        &self,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).assign(project, print, owner, by, at_unix_nanos)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IssueError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this organisation is not yours to read")]
    NotYours,
    #[error("no issue carries that fingerprint")]
    NoSuchIssue,
    #[error("the issues could not be read")]
    Unavailable,
}

impl From<WhoamiError> for IssueError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl From<ReadError> for IssueError {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::StoreUnreachable => Self::Unavailable,
        }
    }
}

#[derive(Debug)]
pub struct Issues<S, G, R, P, C> {
    whoami: Whoami<S, G>,
    reader: R,
    decisions: P,
    clock: C,
}

impl<S, G, R, P, C> Issues<S, G, R, P, C>
where
    S: SessionStore,
    G: Memberships,
    R: IssueReader,
    P: IssueDecisions,
    C: Clock,
{
    pub fn new(whoami: Whoami<S, G>, reader: R, decisions: P, clock: C) -> Self {
        Self {
            whoami,
            reader,
            decisions,
            clock,
        }
    }

    fn now_unix_nanos(&self) -> u64 {
        self.clock.now().get().saturating_mul(NANOS_PER_SECOND)
    }

    async fn acting(&self, token: &SessionToken, org: TenantId) -> Result<AccountId, IssueError> {
        let who = self.whoami.of(token).await?;
        if who.may_act_in(org, OrgRole::Member) {
            Ok(who.account)
        } else {
            Err(IssueError::NotYours)
        }
    }

    pub async fn list(
        &self,
        token: &SessionToken,
        org: TenantId,
        filter: &IssueFilter,
    ) -> Result<Vec<Issue>, IssueError> {
        self.acting(token, org).await?;
        let counted = self.reader.issues(org, filter).await?;
        let keys: Vec<(ProjectId, Fingerprint)> =
            counted.iter().map(|one| (one.project, one.print)).collect();
        let recorded = self.decisions.read_many(&keys).await?;
        Ok(counted
            .into_iter()
            .map(|one| {
                let decision = recorded
                    .iter()
                    .find(|held| held.project == one.project && held.print == one.print)
                    .map(|held| held.decision)
                    .unwrap_or_default();
                Issue::of(one, decision)
            })
            .collect())
    }

    pub async fn one(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: Option<ProjectId>,
        print: Fingerprint,
        window: Window,
    ) -> Result<Issue, IssueError> {
        let mut filter = IssueFilter::over(window).at_most(1);
        filter.project = project;
        filter.print = Some(print);
        self.list(token, org, &filter)
            .await?
            .into_iter()
            .next()
            .ok_or(IssueError::NoSuchIssue)
    }

    pub async fn occurrences(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> Result<Vec<Occurrence>, IssueError> {
        self.acting(token, org).await?;
        Ok(self
            .reader
            .occurrences(org, project, print, window, limit.clamp(1, MAX_LIMIT))
            .await?)
    }

    pub async fn decide(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
    ) -> Result<(), IssueError> {
        let by = self.acting(token, org).await?;
        Ok(self
            .decisions
            .decide(project, print, status, by, self.now_unix_nanos())
            .await?)
    }

    pub async fn assign(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
    ) -> Result<(), IssueError> {
        let by = self.acting(token, org).await?;
        Ok(self
            .decisions
            .assign(project, print, owner, by, self.now_unix_nanos())
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{
        BadFingerprint, Counted, Decision, Fingerprint, Issue, Occurrence, Status, fingerprint,
        normalise,
    };
    use crate::catalogue::ProjectId;
    use crate::identity::AccountId;
    use crate::span::{SpanId, TraceId};

    fn an_occurrence(service: &str, kind: &str, message: &str) -> Occurrence {
        Occurrence {
            service: service.into(),
            exception_type: kind.into(),
            message: message.into(),
            stacktrace: None,
            trace_id: TraceId::try_from([7u8; 16].as_slice()).expect("a trace id"),
            span_id: SpanId::try_from([3u8; 8].as_slice()).expect("a span id"),
            time_unix_nanos: 1,
        }
    }

    fn same(left: &str, right: &str) -> bool {
        fingerprint(&an_occurrence("checkout", "PspTimeoutError", left))
            == fingerprint(&an_occurrence("checkout", "PspTimeoutError", right))
    }

    #[test]
    fn digits_do_not_separate_two_instances_of_one_error() {
        assert!(same(
            "upstream psp-gateway timed out after 3000 ms",
            "upstream psp-gateway timed out after 812 ms"
        ));
        assert!(
            !same(
                "upstream psp-gateway timed out",
                "upstream psp-gateway refused the charge"
            ),
            "different prose is a different error"
        );
    }

    #[test]
    fn a_uuid_or_a_hex_blob_does_not_separate_them() {
        assert!(same(
            "order 6f1c9a52-0e44-4f0a-9f6f-0a5f6f0a5f6f was not found",
            "order 11112222-3333-4444-5555-666677778888 was not found"
        ));
        assert!(same(
            "span 4bf92f3577b34da6 is orphaned",
            "span 45c48cce2e2d7fbd is orphaned"
        ));
        assert!(
            !same("order abc was not found", "order abcd was not found"),
            "a short word is prose, not an identifier"
        );
    }

    #[test]
    fn a_quoted_value_does_not_separate_them() {
        assert!(same(
            "Cannot read properties of undefined (reading 'token')",
            "Cannot read properties of undefined (reading 'session')"
        ));
        assert!(same(
            r#"column "created_at" does not exist"#,
            r#"column "deleted_at" does not exist"#
        ));
    }

    #[test]
    fn only_the_last_segment_of_a_path_is_kept() {
        assert!(same(
            "cannot open /var/lib/comettrace/queue.db",
            "cannot open /tmp/queue.db"
        ));
        assert!(
            !same(
                "cannot open /var/lib/comettrace/queue.db",
                "cannot open /var/lib/comettrace/spool.db"
            ),
            "a different file is a different error"
        );
    }

    #[test]
    fn whitespace_and_case_do_not_separate_them() {
        assert!(same("Pool   exhausted\n", "pool exhausted"));
    }

    #[test]
    fn the_exception_type_separates_two_errors_with_one_message() {
        let timeout = an_occurrence("checkout", "PspTimeoutError", "the call failed");
        let refused = an_occurrence("checkout", "PspRefusedError", "the call failed");

        assert_ne!(fingerprint(&timeout), fingerprint(&refused));
    }

    #[test]
    fn the_service_separates_the_same_exception_in_two_places() {
        let here = an_occurrence("checkout", "PspTimeoutError", "the call failed");
        let there = an_occurrence("payments", "PspTimeoutError", "the call failed");

        assert_ne!(fingerprint(&here), fingerprint(&there));
    }

    #[test]
    fn a_field_boundary_cannot_be_moved_to_collide() {
        let split = an_occurrence("checkout", "Psp", "Timeout the call failed");
        let other = an_occurrence("checkout", "PspTimeout", "the call failed");

        assert_ne!(fingerprint(&split), fingerprint(&other));
    }

    #[test]
    fn a_fingerprint_is_stable_and_sixteen_bytes() {
        let once = fingerprint(&an_occurrence("checkout", "Boom", "it broke 7 times"));
        let twice = fingerprint(&an_occurrence("checkout", "Boom", "it broke 9 times"));

        assert_eq!(once, twice);
        assert_eq!(once.as_bytes().len(), 16);
        assert_eq!(once.to_string().len(), 32);
    }

    #[test]
    fn a_fingerprint_round_trips_through_hex() {
        let print = fingerprint(&an_occurrence("checkout", "Boom", "it broke"));

        let read = Fingerprint::try_from(print.to_string().as_str()).expect("it reads back");

        assert_eq!(read, print);
        assert_eq!(Fingerprint::try_from("abcd"), Err(BadFingerprint));
        assert_eq!(
            Fingerprint::try_from("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"),
            Err(BadFingerprint)
        );
        assert_eq!(Fingerprint::try_from(""), Err(BadFingerprint));
    }

    #[test]
    fn normalising_keeps_the_shape_a_reader_would_recognise() {
        assert_eq!(
            normalise("POST /api/checkout returned 502 after 1200 ms"),
            "post checkout returned 0 after 0 ms"
        );
    }

    fn counted(last_seen: u64) -> Counted {
        Counted {
            project: ProjectId::new(Uuid::from_u128(4)),
            print: fingerprint(&an_occurrence("checkout", "Boom", "it broke")),
            exception_type: "Boom".into(),
            message: "it broke".into(),
            service: "checkout".into(),
            count: 3,
            first_seen_unix_nanos: 10,
            last_seen_unix_nanos: last_seen,
        }
    }

    fn resolved_at(moment: u64) -> Decision {
        Decision {
            status: Status::Resolved,
            owner: None,
            decided_at_unix_nanos: moment,
            decided_by: Some(AccountId::new(Uuid::from_u128(9))),
        }
    }

    #[test]
    fn an_issue_nobody_touched_is_unresolved_and_not_regressed() {
        let issue = Issue::of(counted(20), Decision::default());

        assert_eq!(issue.decision.status, Status::Unresolved);
        assert!(!issue.regressed);
        assert_eq!(issue.decision.decided_by, None);
    }

    #[test]
    fn an_occurrence_after_a_resolution_is_a_regression_that_keeps_its_status() {
        let regressed = Issue::of(counted(300), resolved_at(200));
        let quiet = Issue::of(counted(100), resolved_at(200));

        assert!(regressed.regressed);
        assert_eq!(
            regressed.decision.status,
            Status::Resolved,
            "a regression does not reopen the issue on its own"
        );
        assert!(!quiet.regressed);
    }

    #[test]
    fn a_status_round_trips_through_its_wire_name() {
        for status in [Status::Unresolved, Status::Resolved, Status::Ignored] {
            assert_eq!(Status::parse(status.as_str()), Some(status));
        }
        assert_eq!(Status::parse("wontfix"), None);
    }
}
