use crate::catalogue::{ProjectId, ProjectName};
use crate::secret::ResetToken;
use crate::telemetry::{IngestKey, TenantId};

pub const DEFAULT_PROJECT: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Founded {
    pub org: TenantId,
    pub project: ProjectId,
    pub project_name: ProjectName,
    pub invitation: Option<ResetToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bootstrapped {
    AlreadyFounded { existing: TenantId },
    Founded(Founded),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    pub key: IngestKey,
    pub project: Option<ProjectId>,
}
