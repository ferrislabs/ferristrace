use super::value_objects::{AccountId, Email};
use crate::clock::UnixSeconds;
use crate::profile::Portrait;
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub account: AccountId,
    pub email: Email,
    pub name: Option<Box<str>>,
    pub photo: Option<Portrait>,
    pub org: Option<TenantId>,
    pub expires_at: UnixSeconds,
}
