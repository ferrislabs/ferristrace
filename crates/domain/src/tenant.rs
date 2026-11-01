use core::fmt;
use core::future::Future;

use uuid::Uuid;

use crate::catalogue::ProjectId;
use crate::key::IngestKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TenantId(Uuid);

impl TenantId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the ingest key matches no tenant")]
pub struct UnknownKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    pub tenant: TenantId,
    pub project: Option<ProjectId>,
}

impl Scope {
    pub fn of(tenant: TenantId) -> Self {
        Self {
            tenant,
            project: None,
        }
    }

    #[must_use]
    pub fn in_project(mut self, project: Option<ProjectId>) -> Self {
        self.project = project;
        self
    }
}

pub trait TenantDirectory {
    fn resolve(&self, key: &IngestKey) -> impl Future<Output = Result<Scope, UnknownKey>> + Send;
}

impl<T: TenantDirectory + Sync> TenantDirectory for &T {
    fn resolve(&self, key: &IngestKey) -> impl Future<Output = Result<Scope, UnknownKey>> + Send {
        (*self).resolve(key)
    }
}

impl<T: TenantDirectory + Send + Sync> TenantDirectory for std::sync::Arc<T> {
    fn resolve(&self, key: &IngestKey) -> impl Future<Output = Result<Scope, UnknownKey>> + Send {
        self.as_ref().resolve(key)
    }
}
