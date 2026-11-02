use core::fmt;
use uuid::Uuid;

use super::entities::{Member, Membership};
use super::errors::{BadName, BadTeamName};
use crate::catalogue::ProjectId;
use crate::identity::{AccountId, Email};
use crate::profile::Portrait;
use crate::telemetry::TenantId;

pub(super) const MAX_ORG_NAME_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgName(Box<str>);

impl OrgName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for OrgName {
    type Error = BadName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim();
        if kept.len() > MAX_ORG_NAME_BYTES {
            return Err(BadName::TooLong(MAX_ORG_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadName::Blank);
        }
        Ok(Self(kept.into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrgRole {
    Member,
    Admin,
    Owner,
}

impl OrgRole {
    pub fn at_least(self, floor: Self) -> bool {
        self >= floor
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

impl fmt::Display for OrgRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub account: AccountId,
    pub email: Email,
    pub name: Option<Box<str>>,
    pub photo: Option<Portrait>,
    pub active_org: Option<TenantId>,
    pub memberships: Vec<Membership>,
}

impl Who {
    pub fn role_in(&self, org: TenantId) -> Option<OrgRole> {
        self.memberships
            .iter()
            .find(|membership| membership.org == org)
            .map(|membership| membership.role)
    }

    pub fn may_act_in(&self, org: TenantId, floor: OrgRole) -> bool {
        self.role_in(org).is_some_and(|role| role.at_least(floor))
    }
}

#[derive(Debug)]
pub struct Invitation {
    pub member: Member,
    pub created: bool,
}

pub(super) const MAX_TEAM_NAME_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamName(Box<str>);

impl TeamName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for TeamName {
    type Error = BadTeamName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim();
        if kept.len() > MAX_TEAM_NAME_BYTES {
            return Err(BadTeamName::TooLong(MAX_TEAM_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadTeamName::Blank);
        }
        Ok(Self(kept.into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TeamId(Uuid);

impl TeamId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for TeamId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owned {
    pub id: ProjectId,
    pub name: Box<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    Free,
    Taken,
}
