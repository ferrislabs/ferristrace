use core::future::Future;

use super::entities::{InboxItem, InboxItemDraft};
use super::value_objects::{InboxItemId, NotificationPreferences};
use crate::identity::AccountId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait InboxStore {
    fn list(
        &self,
        account: AccountId,
        org: TenantId,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<InboxItem>, StoreError>> + Send;

    fn create(
        &self,
        draft: &InboxItemDraft,
        at_ms: u64,
    ) -> impl Future<Output = Result<InboxItem, StoreError>> + Send;

    fn mark_read(
        &self,
        account: AccountId,
        org: TenantId,
        id: InboxItemId,
        at_ms: u64,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn mark_all_read(
        &self,
        account: AccountId,
        org: TenantId,
        at_ms: u64,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;

    fn prune(&self, older_than_ms: u64) -> impl Future<Output = Result<u64, StoreError>> + Send;
}

impl<T: InboxStore + Sync> InboxStore for &T {
    fn list(
        &self,
        account: AccountId,
        org: TenantId,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<InboxItem>, StoreError>> + Send {
        (*self).list(account, org, limit)
    }

    fn create(
        &self,
        draft: &InboxItemDraft,
        at_ms: u64,
    ) -> impl Future<Output = Result<InboxItem, StoreError>> + Send {
        (*self).create(draft, at_ms)
    }

    fn mark_read(
        &self,
        account: AccountId,
        org: TenantId,
        id: InboxItemId,
        at_ms: u64,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        (*self).mark_read(account, org, id, at_ms)
    }

    fn mark_all_read(
        &self,
        account: AccountId,
        org: TenantId,
        at_ms: u64,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send {
        (*self).mark_all_read(account, org, at_ms)
    }

    fn prune(&self, older_than_ms: u64) -> impl Future<Output = Result<u64, StoreError>> + Send {
        (*self).prune(older_than_ms)
    }
}

impl<T: InboxStore + Send + Sync> InboxStore for std::sync::Arc<T> {
    fn list(
        &self,
        account: AccountId,
        org: TenantId,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<InboxItem>, StoreError>> + Send {
        self.as_ref().list(account, org, limit)
    }

    fn create(
        &self,
        draft: &InboxItemDraft,
        at_ms: u64,
    ) -> impl Future<Output = Result<InboxItem, StoreError>> + Send {
        self.as_ref().create(draft, at_ms)
    }

    fn mark_read(
        &self,
        account: AccountId,
        org: TenantId,
        id: InboxItemId,
        at_ms: u64,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        self.as_ref().mark_read(account, org, id, at_ms)
    }

    fn mark_all_read(
        &self,
        account: AccountId,
        org: TenantId,
        at_ms: u64,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send {
        self.as_ref().mark_all_read(account, org, at_ms)
    }

    fn prune(&self, older_than_ms: u64) -> impl Future<Output = Result<u64, StoreError>> + Send {
        self.as_ref().prune(older_than_ms)
    }
}

pub trait NotificationSettings {
    fn of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<NotificationPreferences>, StoreError>> + Send;

    fn set(
        &self,
        account: AccountId,
        preferences: &NotificationPreferences,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl<T: NotificationSettings + Sync> NotificationSettings for &T {
    fn of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<NotificationPreferences>, StoreError>> + Send {
        (*self).of(account)
    }

    fn set(
        &self,
        account: AccountId,
        preferences: &NotificationPreferences,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).set(account, preferences)
    }
}

impl<T: NotificationSettings + Send + Sync> NotificationSettings for std::sync::Arc<T> {
    fn of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<NotificationPreferences>, StoreError>> + Send {
        self.as_ref().of(account)
    }

    fn set(
        &self,
        account: AccountId,
        preferences: &NotificationPreferences,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().set(account, preferences)
    }
}
