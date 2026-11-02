use core::future::Future;

use super::entities::{ApiKey, IssuedKey};
use super::value_objects::{ApiSecret, KeyId, KeyName, KeyScopes, ReadGrant};
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
}
