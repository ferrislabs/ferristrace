use core::fmt;
use uuid::Uuid;

use crate::access::KeyId;
use crate::catalogue::ProjectId;
use crate::identity::AccountId;
use crate::issue::Fingerprint;
use crate::organisation::TeamId;
use crate::telemetry::TenantId;

pub const DEFAULT_PAGE: u32 = 50;
pub const MAX_PAGE: u32 = 200;

const AT_DIGITS: usize = 16;
const ID_DIGITS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(Uuid);

impl EntryId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Account(AccountId),
    ApiKey(KeyId),
    Operator,
}

impl Actor {
    pub fn kind(self) -> &'static str {
        match self {
            Self::Account(_) => "account",
            Self::ApiKey(_) => "api_key",
            Self::Operator => "operator",
        }
    }

    pub fn id(self) -> Option<Uuid> {
        match self {
            Self::Account(account) => Some(account.as_uuid()),
            Self::ApiKey(key) => Some(key.as_uuid()),
            Self::Operator => None,
        }
    }

    pub fn of(kind: &str, id: Option<Uuid>) -> Option<Self> {
        match (kind, id) {
            ("account", Some(id)) => Some(Self::Account(AccountId::new(id))),
            ("api_key", Some(id)) => Some(Self::ApiKey(KeyId::new(id))),
            ("operator", None) => Some(Self::Operator),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    OrgFounded,
    OrgRenamed,
    OrgLogoSet,
    OrgLogoRemoved,
    OrgRetentionSet,
    OrgExported,
    ProjectCreated,
    ProjectRenamed,
    ProjectDeleted,
    ProjectAssigned,
    TeamCreated,
    TeamRenamed,
    TeamDisbanded,
    TeamSeated,
    TeamProjectsOwned,
    MemberInvited,
    MemberRoleGranted,
    MemberRemoved,
    KeyIssued,
    KeyRevoked,
    IssueDecided,
    IssueAssigned,
    AccountCreatedByOperator,
    ResetLinkIssuedByOperator,
    AlertCreated,
    AlertUpdated,
    AlertDeleted,
    AlertEnabled,
    AlertDisabled,
    ChannelCreated,
    ChannelUpdated,
    ChannelDeleted,
}

impl Action {
    pub const ALL: [Self; 32] = [
        Self::OrgFounded,
        Self::OrgRenamed,
        Self::OrgLogoSet,
        Self::OrgLogoRemoved,
        Self::OrgRetentionSet,
        Self::OrgExported,
        Self::ProjectCreated,
        Self::ProjectRenamed,
        Self::ProjectDeleted,
        Self::ProjectAssigned,
        Self::TeamCreated,
        Self::TeamRenamed,
        Self::TeamDisbanded,
        Self::TeamSeated,
        Self::TeamProjectsOwned,
        Self::MemberInvited,
        Self::MemberRoleGranted,
        Self::MemberRemoved,
        Self::KeyIssued,
        Self::KeyRevoked,
        Self::IssueDecided,
        Self::IssueAssigned,
        Self::AccountCreatedByOperator,
        Self::ResetLinkIssuedByOperator,
        Self::AlertCreated,
        Self::AlertUpdated,
        Self::AlertDeleted,
        Self::AlertEnabled,
        Self::AlertDisabled,
        Self::ChannelCreated,
        Self::ChannelUpdated,
        Self::ChannelDeleted,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OrgFounded => "org.founded",
            Self::OrgRenamed => "org.renamed",
            Self::OrgLogoSet => "org.logo_set",
            Self::OrgLogoRemoved => "org.logo_removed",
            Self::OrgRetentionSet => "org.retention_set",
            Self::OrgExported => "org.exported",
            Self::ProjectCreated => "project.created",
            Self::ProjectRenamed => "project.renamed",
            Self::ProjectDeleted => "project.deleted",
            Self::ProjectAssigned => "project.assigned",
            Self::TeamCreated => "team.created",
            Self::TeamRenamed => "team.renamed",
            Self::TeamDisbanded => "team.disbanded",
            Self::TeamSeated => "team.seated",
            Self::TeamProjectsOwned => "team.projects_owned",
            Self::MemberInvited => "member.invited",
            Self::MemberRoleGranted => "member.role_granted",
            Self::MemberRemoved => "member.removed",
            Self::KeyIssued => "key.issued",
            Self::KeyRevoked => "key.revoked",
            Self::IssueDecided => "issue.decided",
            Self::IssueAssigned => "issue.assigned",
            Self::AccountCreatedByOperator => "account.created_by_operator",
            Self::ResetLinkIssuedByOperator => "reset_link.issued_by_operator",
            Self::AlertCreated => "alert.created",
            Self::AlertUpdated => "alert.updated",
            Self::AlertDeleted => "alert.deleted",
            Self::AlertEnabled => "alert.enabled",
            Self::AlertDisabled => "alert.disabled",
            Self::ChannelCreated => "channel.created",
            Self::ChannelUpdated => "channel.updated",
            Self::ChannelDeleted => "channel.deleted",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.as_str() == text)
    }
}

impl fmt::Display for Action {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetKind {
    Org,
    Project,
    Team,
    Member,
    Account,
    Key,
    Issue,
    Alert,
    Channel,
}

impl TargetKind {
    const ALL: [Self; 9] = [
        Self::Org,
        Self::Project,
        Self::Team,
        Self::Member,
        Self::Account,
        Self::Key,
        Self::Issue,
        Self::Alert,
        Self::Channel,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Org => "org",
            Self::Project => "project",
            Self::Team => "team",
            Self::Member => "member",
            Self::Account => "account",
            Self::Key => "key",
            Self::Issue => "issue",
            Self::Alert => "alert",
            Self::Channel => "channel",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub kind: TargetKind,
    pub id: Option<Box<str>>,
}

impl Target {
    pub fn of(kind: TargetKind, id: Option<Box<str>>) -> Self {
        Self { kind, id }
    }

    fn named(kind: TargetKind, id: impl fmt::Display) -> Self {
        Self {
            kind,
            id: Some(id.to_string().into()),
        }
    }

    pub fn org(org: TenantId) -> Self {
        Self::named(TargetKind::Org, org)
    }

    pub fn project(project: ProjectId) -> Self {
        Self::named(TargetKind::Project, project)
    }

    pub fn team(team: TeamId) -> Self {
        Self::named(TargetKind::Team, team)
    }

    pub fn member(account: AccountId) -> Self {
        Self::named(TargetKind::Member, account)
    }

    pub fn account(account: AccountId) -> Self {
        Self::named(TargetKind::Account, account)
    }

    pub fn key(key: KeyId) -> Self {
        Self::named(TargetKind::Key, key)
    }

    pub fn issue(print: Fingerprint) -> Self {
        Self::named(TargetKind::Issue, print)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detail(Vec<(Box<str>, Option<Box<str>>)>);

impl Detail {
    pub fn from_pairs(pairs: Vec<(Box<str>, Option<Box<str>>)>) -> Self {
        Self(pairs)
    }

    pub fn with(self, key: &str, value: impl fmt::Display) -> Self {
        self.with_maybe(key, Some(value))
    }

    pub fn with_maybe(mut self, key: &str, value: Option<impl fmt::Display>) -> Self {
        self.0.retain(|(held, _)| &**held != key);
        self.0
            .push((key.into(), value.map(|value| value.to_string().into())));
        self
    }

    pub fn with_all<T: fmt::Display>(self, key: &str, values: &[T]) -> Self {
        let joined = values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        self.with(key, joined)
    }

    pub fn get(&self, key: &str) -> Option<Option<&str>> {
        self.0
            .iter()
            .find(|(held, _)| &**held == key)
            .map(|(_, value)| value.as_deref())
    }

    pub fn pairs(&self) -> &[(Box<str>, Option<Box<str>>)] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub at_unix_nanos: u64,
    pub id: EntryId,
}

impl Cursor {
    pub fn token(&self) -> String {
        format!(
            "{:0width$x}{}",
            self.at_unix_nanos,
            self.id.as_uuid().simple(),
            width = AT_DIGITS
        )
    }

    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != AT_DIGITS + ID_DIGITS || !text.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        let (at, id) = text.split_at(AT_DIGITS);
        Some(Self {
            at_unix_nanos: u64::from_str_radix(at, 16).ok()?,
            id: EntryId::new(Uuid::try_parse(id).ok()?),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSize(u32);

impl PageSize {
    pub fn new(asked: Option<u32>) -> Self {
        Self(asked.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE))
    }

    pub fn get(self) -> u32 {
        self.0
    }

    pub fn probe(self) -> u32 {
        self.0 + 1
    }
}

impl Default for PageSize {
    fn default() -> Self {
        Self::new(None)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuditFilter {
    pub actor: Option<Actor>,
    pub action: Option<Action>,
    pub before: Option<Cursor>,
}
