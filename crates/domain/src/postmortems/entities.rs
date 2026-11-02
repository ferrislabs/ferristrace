use super::value_objects::{PostmortemBody, PostmortemId, PostmortemStatus};
use crate::identity::AccountId;
use crate::incidents::{IncidentId, Person};
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Postmortem {
    pub id: PostmortemId,
    pub org: TenantId,
    pub incident: IncidentId,
    pub status: PostmortemStatus,
    pub body: PostmortemBody,
    pub author: Option<AccountId>,
    pub edited: bool,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub published_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub postmortem: Postmortem,
    pub author: Option<Person>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostmortemSummary {
    pub incident: IncidentId,
    pub status: PostmortemStatus,
}
