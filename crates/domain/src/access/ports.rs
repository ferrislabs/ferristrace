use core::future::Future;

use super::entities::{ApiKey, IssuedKey};
use super::value_objects::{ApiSecret, KeyId, KeyName, KeyScopes, ReadGrant};
use crate::agents::{Agent, AgentId};
use crate::audit::AuditEntry;
use crate::catalogue::ProjectId;
use crate::identity::AccountId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait ApiKeys {
    fn list(&self, org: TenantId) -> impl Future<Output = Result<Vec<ApiKey>, StoreError>> + Send;

    fn issue(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        by: AccountId,
    ) -> impl Future<Output = Result<IssuedKey, StoreError>> + Send;

    fn revoke(
        &self,
        org: TenantId,
        id: KeyId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn grant_for(
        &self,
        secret: &ApiSecret,
    ) -> impl Future<Output = Result<Option<ReadGrant>, StoreError>> + Send;

    fn agent_standing(
        &self,
        _org: TenantId,
        _client_id: &str,
    ) -> impl Future<Output = Result<Option<Agent>, StoreError>> + Send {
        async { Ok(None) }
    }

    fn agent_used(
        &self,
        _org: TenantId,
        _agent: AgentId,
        _entry: &AuditEntry,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        async { Ok(()) }
    }
}

impl<T: ApiKeys + Sync> ApiKeys for &T {
    fn list(&self, org: TenantId) -> impl Future<Output = Result<Vec<ApiKey>, StoreError>> + Send {
        (*self).list(org)
    }

    fn issue(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        by: AccountId,
    ) -> impl Future<Output = Result<IssuedKey, StoreError>> + Send {
        (*self).issue(org, project, name, scopes, by)
    }

    fn revoke(
        &self,
        org: TenantId,
        id: KeyId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        (*self).revoke(org, id)
    }

    fn grant_for(
        &self,
        secret: &ApiSecret,
    ) -> impl Future<Output = Result<Option<ReadGrant>, StoreError>> + Send {
        (*self).grant_for(secret)
    }

    fn agent_standing(
        &self,
        org: TenantId,
        client_id: &str,
    ) -> impl Future<Output = Result<Option<Agent>, StoreError>> + Send {
        (*self).agent_standing(org, client_id)
    }

    fn agent_used(
        &self,
        org: TenantId,
        agent: AgentId,
        entry: &AuditEntry,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).agent_used(org, agent, entry)
    }
}

impl<T: ApiKeys + Send + Sync> ApiKeys for std::sync::Arc<T> {
    fn list(&self, org: TenantId) -> impl Future<Output = Result<Vec<ApiKey>, StoreError>> + Send {
        self.as_ref().list(org)
    }

    fn issue(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        by: AccountId,
    ) -> impl Future<Output = Result<IssuedKey, StoreError>> + Send {
        self.as_ref().issue(org, project, name, scopes, by)
    }

    fn revoke(
        &self,
        org: TenantId,
        id: KeyId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        self.as_ref().revoke(org, id)
    }

    fn grant_for(
        &self,
        secret: &ApiSecret,
    ) -> impl Future<Output = Result<Option<ReadGrant>, StoreError>> + Send {
        self.as_ref().grant_for(secret)
    }

    fn agent_standing(
        &self,
        org: TenantId,
        client_id: &str,
    ) -> impl Future<Output = Result<Option<Agent>, StoreError>> + Send {
        self.as_ref().agent_standing(org, client_id)
    }

    fn agent_used(
        &self,
        org: TenantId,
        agent: AgentId,
        entry: &AuditEntry,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().agent_used(org, agent, entry)
    }
}
