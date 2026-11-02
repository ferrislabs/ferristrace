use core::future::Future;

use super::entities::{
    AlertEvent, Channel, ChannelView, Claimed, Dispatch, Notification, Observation, Rule,
    RuleDraft, SeriesState, Settlement,
};
use super::errors::{DeliveryError, ProbeError};
use super::value_objects::{
    ChannelId, ChannelName, Destination, Duty, EmailList, NotificationId, RuleId, Signal,
    WebhookSecret, WebhookUrl,
};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::identity::AccountId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait AlertRules {
    fn list(&self, org: TenantId) -> impl Future<Output = Result<Vec<Rule>, StoreError>> + Send;

    fn get(
        &self,
        org: TenantId,
        id: RuleId,
    ) -> impl Future<Output = Result<Option<Rule>, StoreError>> + Send;

    fn create(
        &self,
        org: TenantId,
        draft: &RuleDraft,
        by: AccountId,
    ) -> impl Future<Output = Result<Option<Rule>, StoreError>> + Send;

    fn update(&self, rule: &Rule) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn delete(
        &self,
        org: TenantId,
        id: RuleId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn enabled(&self) -> impl Future<Output = Result<Vec<Rule>, StoreError>> + Send;
}

pub trait AlertStates {
    fn of(&self, rule: RuleId)
    -> impl Future<Output = Result<Vec<SeriesState>, StoreError>> + Send;

    fn of_org(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Vec<(RuleId, SeriesState)>, StoreError>> + Send;

    fn keep(
        &self,
        rule: RuleId,
        state: &SeriesState,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

pub trait AlertEvents {
    fn record(&self, event: &AlertEvent) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn history(
        &self,
        org: TenantId,
        rule: RuleId,
        from_ms: u64,
        to_ms: u64,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<AlertEvent>, StoreError>> + Send;

    fn prune(&self, before_ms: u64) -> impl Future<Output = Result<u64, StoreError>> + Send;
}

pub trait AlertOutbox {
    fn enqueue(
        &self,
        notification: &Notification,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn claim(
        &self,
        now_ms: u64,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Claimed>, StoreError>> + Send;

    fn settle(
        &self,
        id: NotificationId,
        attempts: u32,
        settlement: &Settlement,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

pub trait AlertChannels {
    fn list(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Vec<ChannelView>, StoreError>> + Send;

    fn armed(
        &self,
        org: TenantId,
        id: ChannelId,
    ) -> impl Future<Output = Result<Option<Channel>, StoreError>> + Send;

    fn create(
        &self,
        org: TenantId,
        name: &ChannelName,
        destination: &Destination,
        secret: Option<&WebhookSecret>,
        by: AccountId,
    ) -> impl Future<Output = Result<Option<ChannelView>, StoreError>> + Send;

    fn update(
        &self,
        org: TenantId,
        id: ChannelId,
        name: &ChannelName,
        destination: &Destination,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn delete(
        &self,
        org: TenantId,
        id: ChannelId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    fn keeps_secrets(&self) -> bool;
}

pub trait AlertDuty {
    fn lead(&self, duty: Duty, slot: u64) -> impl Future<Output = Result<bool, StoreError>> + Send;
}

pub trait SignalProbe {
    fn observe(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        signal: &Signal,
        window: Window,
    ) -> impl Future<Output = Result<Vec<Observation>, ProbeError>> + Send;
}

pub trait WebhookSender {
    fn admits(&self, url: &WebhookUrl) -> bool;

    fn send(
        &self,
        url: &WebhookUrl,
        secret: &WebhookSecret,
        dispatch: &Dispatch,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send;
}

impl<T: WebhookSender + Sync> WebhookSender for &T {
    fn admits(&self, url: &WebhookUrl) -> bool {
        (*self).admits(url)
    }

    fn send(
        &self,
        url: &WebhookUrl,
        secret: &WebhookSecret,
        dispatch: &Dispatch,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        (*self).send(url, secret, dispatch)
    }
}

impl<T: WebhookSender + Send + Sync> WebhookSender for std::sync::Arc<T> {
    fn admits(&self, url: &WebhookUrl) -> bool {
        self.as_ref().admits(url)
    }

    fn send(
        &self,
        url: &WebhookUrl,
        secret: &WebhookSecret,
        dispatch: &Dispatch,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        self.as_ref().send(url, secret, dispatch)
    }
}

pub trait AlertMailer {
    fn send_alert(
        &self,
        to: &EmailList,
        dispatch: &Dispatch,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send;
}

impl<T: AlertMailer + Sync> AlertMailer for &T {
    fn send_alert(
        &self,
        to: &EmailList,
        dispatch: &Dispatch,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        (*self).send_alert(to, dispatch)
    }
}

impl<T: AlertMailer + Send + Sync> AlertMailer for std::sync::Arc<T> {
    fn send_alert(
        &self,
        to: &EmailList,
        dispatch: &Dispatch,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        self.as_ref().send_alert(to, dispatch)
    }
}
