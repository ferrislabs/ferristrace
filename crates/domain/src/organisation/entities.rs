use super::value_objects::{OrgRole, Owned, TeamId};
use crate::identity::{AccountId, Email};
use crate::profile::Portrait;
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organisation {
    pub id: TenantId,
    pub name: Box<str>,
    pub plan: Box<str>,
    pub created_at_unix_nanos: u64,
    pub logo: Option<Portrait>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    pub org: TenantId,
    pub name: Box<str>,
    pub role: OrgRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub account: AccountId,
    pub email: Email,
    pub name: Option<Box<str>>,
    pub photo: Option<Portrait>,
    pub role: OrgRole,
    pub joined_at_unix_nanos: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    pub id: TeamId,
    pub name: Box<str>,
    pub created_at_unix_nanos: u64,
    pub members: Vec<AccountId>,
    pub projects: Vec<Owned>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub account: AccountId,
    pub name: Option<Box<str>>,
    pub photo: Option<Portrait>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Joined {
    pub role: OrgRole,
    pub joined_at_unix_nanos: u64,
}
