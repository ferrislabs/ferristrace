use core::future::Future;

use super::entities::{
    Announcement, ClaimedLetter, Component, Invite, Letter, Saved, StatusPage, Subscriber,
};
use super::value_objects::{ComponentId, LetterId, Slug, StatusPageId, SubscriberId};
use crate::alerts::{DeliveryError, Settlement};
use crate::identity::Email;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait StatusPages {
    fn list(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Vec<StatusPage>, StoreError>> + Send;

    fn get(
        &self,
        org: TenantId,
        id: StatusPageId,
    ) -> impl Future<Output = Result<Option<StatusPage>, StoreError>> + Send;

    fn by_slug(
        &self,
        slug: &Slug,
    ) -> impl Future<Output = Result<Option<StatusPage>, StoreError>> + Send;

    fn by_id(
        &self,
        id: StatusPageId,
    ) -> impl Future<Output = Result<Option<StatusPage>, StoreError>> + Send;

    fn create(&self, page: &StatusPage) -> impl Future<Output = Result<Saved, StoreError>> + Send;

    fn keep(&self, page: &StatusPage) -> impl Future<Output = Result<Saved, StoreError>> + Send;

    fn delete(
        &self,
        org: TenantId,
        id: StatusPageId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn components(
        &self,
        org: TenantId,
        page: StatusPageId,
    ) -> impl Future<Output = Result<Vec<Component>, StoreError>> + Send;

    fn add_component(
        &self,
        org: TenantId,
        component: &Component,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn keep_component(
        &self,
        org: TenantId,
        component: &Component,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn remove_component(
        &self,
        org: TenantId,
        page: StatusPageId,
        id: ComponentId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
}

pub trait StatusSubscribers {
    fn find(
        &self,
        page: StatusPageId,
        email: &Email,
    ) -> impl Future<Output = Result<Option<Subscriber>, StoreError>> + Send;

    fn get(
        &self,
        id: SubscriberId,
    ) -> impl Future<Output = Result<Option<Subscriber>, StoreError>> + Send;

    fn add(&self, subscriber: &Subscriber)
    -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn keep(
        &self,
        subscriber: &Subscriber,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn remove(&self, id: SubscriberId) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn confirmed(&self, page: StatusPageId)
    -> impl Future<Output = Result<u64, StoreError>> + Send;
}

pub trait StatusOutbox {
    fn announce(
        &self,
        announcement: &Announcement,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;

    fn invite(&self, invite: &Invite) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn claim(
        &self,
        now_ms: u64,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<ClaimedLetter>, StoreError>> + Send;

    fn settle(
        &self,
        id: LetterId,
        attempts: u32,
        settlement: &Settlement,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

pub trait StatusMailer {
    fn send_status(
        &self,
        to: &Email,
        letter: &Letter,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send;
}

impl<T: StatusMailer + Sync> StatusMailer for &T {
    fn send_status(
        &self,
        to: &Email,
        letter: &Letter,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        (*self).send_status(to, letter)
    }
}

impl<T: StatusMailer + Send + Sync> StatusMailer for std::sync::Arc<T> {
    fn send_status(
        &self,
        to: &Email,
        letter: &Letter,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        self.as_ref().send_status(to, letter)
    }
}
