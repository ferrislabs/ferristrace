use crate::access::KeyId;
use crate::catalogue::{ProjectId, ProjectName};
use crate::telemetry::{IngestKey, TenantId};

pub const DEFAULT_PROJECT: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Founded {
    pub org: TenantId,
    pub project: ProjectId,
    pub project_name: ProjectName,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Minted {
    pub id: KeyId,
    pub key: IngestKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revoked {
    pub org: TenantId,
    pub id: KeyId,
}
