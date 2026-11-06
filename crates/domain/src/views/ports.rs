use core::future::Future;

use super::entities::{NewView, View, ViewChange};
use super::value_objects::{ViewId, ViewSignal};
use crate::identity::AccountId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Creation {
    Made(View),
    NameTaken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saving {
    Saved,
    Missing,
    NameTaken,
}

pub trait ExplorerViews {
    fn visible(
        &self,
        org: TenantId,
        signal: ViewSignal,
        to: AccountId,
    ) -> impl Future<Output = Result<Vec<View>, StoreError>> + Send;

    fn find(
        &self,
        org: TenantId,
        id: ViewId,
    ) -> impl Future<Output = Result<Option<View>, StoreError>> + Send;

    fn create(
        &self,
        org: TenantId,
        by: AccountId,
        view: &NewView,
    ) -> impl Future<Output = Result<Creation, StoreError>> + Send;

    fn change(
        &self,
        org: TenantId,
        id: ViewId,
        change: &ViewChange,
    ) -> impl Future<Output = Result<Saving, StoreError>> + Send;

    fn delete(
        &self,
        org: TenantId,
        id: ViewId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
}

impl<T: ExplorerViews + Sync> ExplorerViews for &T {
    fn visible(
        &self,
        org: TenantId,
        signal: ViewSignal,
        to: AccountId,
    ) -> impl Future<Output = Result<Vec<View>, StoreError>> + Send {
        (*self).visible(org, signal, to)
    }

    fn find(
        &self,
        org: TenantId,
        id: ViewId,
    ) -> impl Future<Output = Result<Option<View>, StoreError>> + Send {
        (*self).find(org, id)
    }

    fn create(
        &self,
        org: TenantId,
        by: AccountId,
        view: &NewView,
    ) -> impl Future<Output = Result<Creation, StoreError>> + Send {
        (*self).create(org, by, view)
    }

    fn change(
        &self,
        org: TenantId,
        id: ViewId,
        change: &ViewChange,
    ) -> impl Future<Output = Result<Saving, StoreError>> + Send {
        (*self).change(org, id, change)
    }

    fn delete(
        &self,
        org: TenantId,
        id: ViewId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        (*self).delete(org, id)
    }
}

impl<T: ExplorerViews + Send + Sync> ExplorerViews for std::sync::Arc<T> {
    fn visible(
        &self,
        org: TenantId,
        signal: ViewSignal,
        to: AccountId,
    ) -> impl Future<Output = Result<Vec<View>, StoreError>> + Send {
        self.as_ref().visible(org, signal, to)
    }

    fn find(
        &self,
        org: TenantId,
        id: ViewId,
    ) -> impl Future<Output = Result<Option<View>, StoreError>> + Send {
        self.as_ref().find(org, id)
    }

    fn create(
        &self,
        org: TenantId,
        by: AccountId,
        view: &NewView,
    ) -> impl Future<Output = Result<Creation, StoreError>> + Send {
        self.as_ref().create(org, by, view)
    }

    fn change(
        &self,
        org: TenantId,
        id: ViewId,
        change: &ViewChange,
    ) -> impl Future<Output = Result<Saving, StoreError>> + Send {
        self.as_ref().change(org, id, change)
    }

    fn delete(
        &self,
        org: TenantId,
        id: ViewId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        self.as_ref().delete(org, id)
    }
}
