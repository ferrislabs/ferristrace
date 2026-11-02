use super::value_objects::{OwnerTeam, ProjectId};
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: ProjectId,
    pub org: TenantId,
    pub name: Box<str>,
    pub created_at_unix_nanos: u64,
    pub team: Option<OwnerTeam>,
    pub keys: u32,
}
