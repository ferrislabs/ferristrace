use core::future::Future;

use super::entities::{ApiKey, ExpiringKey, IssuedKey};
use super::value_objects::{
    ApiSecret, ExpiryThreshold, KeyId, KeyName, KeyScopes, KeyTtl, ReadGrant,
};
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
        ttl: KeyTtl,
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

    /// Every key within a notice threshold of its deadline that has not
    /// already been told about that threshold - never a revoked key.
    fn due_for_expiry_notice(
        &self,
        _now_unix_nanos: u64,
    ) -> impl Future<Output = Result<Vec<(ExpiringKey, ExpiryThreshold)>, StoreError>> + Send {
        async { Ok(Vec::new()) }
    }

    fn mark_expiry_notice_sent(
        &self,
        _id: KeyId,
        _threshold: ExpiryThreshold,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        async { Ok(()) }
    }

    /// Every live key whose deadline has already passed.
    fn due_for_auto_revoke(
        &self,
        _now_unix_nanos: u64,
    ) -> impl Future<Output = Result<Vec<ExpiringKey>, StoreError>> + Send {
        async { Ok(Vec::new()) }
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
        ttl: KeyTtl,
    ) -> impl Future<Output = Result<IssuedKey, StoreError>> + Send {
        (*self).issue(org, project, name, scopes, by, ttl)
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

    fn due_for_expiry_notice(
        &self,
        now_unix_nanos: u64,
    ) -> impl Future<Output = Result<Vec<(ExpiringKey, ExpiryThreshold)>, StoreError>> + Send {
        (*self).due_for_expiry_notice(now_unix_nanos)
    }

    fn mark_expiry_notice_sent(
        &self,
        id: KeyId,
        threshold: ExpiryThreshold,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).mark_expiry_notice_sent(id, threshold)
    }

    fn due_for_auto_revoke(
        &self,
        now_unix_nanos: u64,
    ) -> impl Future<Output = Result<Vec<ExpiringKey>, StoreError>> + Send {
        (*self).due_for_auto_revoke(now_unix_nanos)
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
        ttl: KeyTtl,
    ) -> impl Future<Output = Result<IssuedKey, StoreError>> + Send {
        self.as_ref().issue(org, project, name, scopes, by, ttl)
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

    fn due_for_expiry_notice(
        &self,
        now_unix_nanos: u64,
    ) -> impl Future<Output = Result<Vec<(ExpiringKey, ExpiryThreshold)>, StoreError>> + Send {
        self.as_ref().due_for_expiry_notice(now_unix_nanos)
    }

    fn mark_expiry_notice_sent(
        &self,
        id: KeyId,
        threshold: ExpiryThreshold,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().mark_expiry_notice_sent(id, threshold)
    }

    fn due_for_auto_revoke(
        &self,
        now_unix_nanos: u64,
    ) -> impl Future<Output = Result<Vec<ExpiringKey>, StoreError>> + Send {
        self.as_ref().due_for_auto_revoke(now_unix_nanos)
    }
}
