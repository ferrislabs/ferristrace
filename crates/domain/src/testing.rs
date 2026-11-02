use std::sync::{Arc, Mutex};

use uuid::Uuid;

use crate::audit::{
    Actor, AuditEntry, AuditFilter, AuditPage, AuditTrail, EntryId, Logged, PageSize,
};
use crate::store::StoreError;
use crate::telemetry::IngestKey;
use crate::telemetry::{TenantDirectory, TenantId, UnknownKey};

pub async fn a_directory_resolves_the_key_it_holds<D: TenantDirectory>(
    directory: &D,
    held: &IngestKey,
    tenant: TenantId,
) {
    let resolved = directory.resolve(held).await;

    assert_eq!(resolved.map(|scope| scope.tenant), Ok(tenant));
}

pub async fn a_directory_refuses_a_key_it_does_not_hold<D: TenantDirectory>(
    directory: &D,
    absent: &IngestKey,
) {
    let resolved = directory.resolve(absent).await;

    assert_eq!(resolved.map(|scope| scope.tenant), Err(UnknownKey));
}

pub async fn a_directory_answers_the_same_key_the_same_way_twice<D: TenantDirectory>(
    directory: &D,
    held: &IngestKey,
) {
    let once = directory.resolve(held).await;
    let twice = directory.resolve(held).await;

    assert_eq!(once, twice);
}

#[derive(Debug)]
struct Labelled {
    actor: Actor,
    label: Box<str>,
}

#[derive(Debug, Clone, Default)]
pub struct Notebook {
    logged: Arc<Mutex<Vec<Logged>>>,
    labels: Arc<Mutex<Vec<Labelled>>>,
}

impl Notebook {
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.logged
            .lock()
            .expect("the notebook lock")
            .iter()
            .map(|logged| logged.entry.clone())
            .collect()
    }

    pub fn label(&self, actor: Actor, label: &str) {
        self.labels
            .lock()
            .expect("the notebook lock")
            .push(Labelled {
                actor,
                label: label.into(),
            });
    }
}

impl AuditTrail for Notebook {
    async fn record(&self, entry: &AuditEntry) -> Result<(), StoreError> {
        let mut logged = self.logged.lock().expect("the notebook lock");
        let seed = u64::try_from(logged.len()).unwrap_or(u64::MAX) + 1;
        let label = self
            .labels
            .lock()
            .expect("the notebook lock")
            .iter()
            .find(|held| held.actor == entry.actor)
            .map(|held| held.label.clone());
        logged.push(Logged {
            id: EntryId::new(Uuid::from_u128(u128::from(seed))),
            at_unix_nanos: seed,
            entry: entry.clone(),
            label,
        });
        Ok(())
    }

    async fn list(
        &self,
        org: TenantId,
        filter: &AuditFilter,
        page: PageSize,
    ) -> Result<AuditPage, StoreError> {
        let probed = self
            .logged
            .lock()
            .expect("the notebook lock")
            .iter()
            .rev()
            .filter(|logged| logged.entry.org == org)
            .filter(|logged| filter.actor.is_none_or(|actor| logged.entry.actor == actor))
            .filter(|logged| {
                filter
                    .action
                    .is_none_or(|action| logged.entry.action == action)
            })
            .filter(|logged| {
                filter.before.is_none_or(|before| {
                    (logged.at_unix_nanos, logged.id) < (before.at_unix_nanos, before.id)
                })
            })
            .take(usize::try_from(page.probe()).unwrap_or(usize::MAX))
            .cloned()
            .collect();
        Ok(AuditPage::cut(probed, page))
    }
}

#[derive(Debug, Default)]
struct Held {
    rules: Vec<crate::alerts::Rule>,
    states: Vec<(crate::alerts::RuleId, crate::alerts::SeriesState)>,
    events: Vec<crate::alerts::AlertEvent>,
    outbox: Vec<crate::alerts::Notification>,
    channels: Vec<(TenantId, crate::alerts::Channel)>,
    slots: Vec<(crate::alerts::Duty, u64)>,
    busy: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AlertLedger {
    held: Arc<Mutex<Held>>,
    sealed: bool,
}

impl AlertLedger {
    pub fn sealed() -> Self {
        Self {
            held: Arc::default(),
            sealed: true,
        }
    }

    fn with<T>(&self, work: impl FnOnce(&mut Held) -> T) -> T {
        work(&mut self.held.lock().expect("the ledger lock"))
    }

    pub fn rules(&self) -> Vec<crate::alerts::Rule> {
        self.with(|held| held.rules.clone())
    }

    pub fn events(&self) -> Vec<crate::alerts::AlertEvent> {
        self.with(|held| held.events.clone())
    }

    pub fn notifications(&self) -> Vec<crate::alerts::Notification> {
        self.with(|held| held.outbox.clone())
    }

    pub fn states(&self) -> Vec<(crate::alerts::RuleId, crate::alerts::SeriesState)> {
        self.with(|held| held.states.clone())
    }

    pub fn secret_of(&self, id: crate::alerts::ChannelId) -> Option<crate::alerts::WebhookSecret> {
        self.with(|held| {
            held.channels
                .iter()
                .find(|(_, channel)| channel.id == id)
                .and_then(|(_, channel)| channel.secret.clone())
        })
    }

    pub fn hold_the_lock(&self, busy: bool) {
        self.with(|held| held.busy = busy);
    }

    pub fn put_event(&self, event: crate::alerts::AlertEvent) {
        self.with(|held| held.events.push(event));
    }
}

fn view_of(channel: &crate::alerts::Channel) -> crate::alerts::ChannelView {
    crate::alerts::ChannelView {
        id: channel.id,
        name: channel.name.clone(),
        destination: channel.destination.clone(),
        has_secret: channel.secret.is_some(),
    }
}

impl crate::alerts::AlertRules for AlertLedger {
    async fn list(&self, org: TenantId) -> Result<Vec<crate::alerts::Rule>, StoreError> {
        Ok(self.with(|held| {
            held.rules
                .iter()
                .filter(|rule| rule.org == org)
                .cloned()
                .collect()
        }))
    }

    async fn get(
        &self,
        org: TenantId,
        id: crate::alerts::RuleId,
    ) -> Result<Option<crate::alerts::Rule>, StoreError> {
        Ok(self.with(|held| {
            held.rules
                .iter()
                .find(|rule| rule.org == org && rule.id == id)
                .cloned()
        }))
    }

    async fn create(
        &self,
        org: TenantId,
        draft: &crate::alerts::RuleDraft,
        _by: crate::identity::AccountId,
    ) -> Result<Option<crate::alerts::Rule>, StoreError> {
        Ok(self.with(|held| {
            if held
                .rules
                .iter()
                .any(|rule| rule.org == org && rule.name == draft.name)
            {
                return None;
            }
            let rule = crate::alerts::Rule {
                id: crate::alerts::RuleId::fresh(),
                org,
                name: draft.name.clone(),
                project: draft.project,
                enabled: draft.enabled,
                definition: draft.definition.clone(),
                created_ms: 0,
            };
            held.rules.push(rule.clone());
            Some(rule)
        }))
    }

    async fn update(&self, rule: &crate::alerts::Rule) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            held.rules
                .iter_mut()
                .find(|kept| kept.org == rule.org && kept.id == rule.id)
                .map(|kept| *kept = rule.clone())
                .is_some()
        }))
    }

    async fn delete(&self, org: TenantId, id: crate::alerts::RuleId) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            let before = held.rules.len();
            held.rules
                .retain(|rule| !(rule.org == org && rule.id == id));
            held.states.retain(|(rule, _)| *rule != id);
            held.events.retain(|event| event.rule != id);
            held.outbox.retain(|notification| notification.rule != id);
            held.rules.len() != before
        }))
    }

    async fn enabled(&self) -> Result<Vec<crate::alerts::Rule>, StoreError> {
        Ok(self.with(|held| {
            held.rules
                .iter()
                .filter(|rule| rule.enabled)
                .cloned()
                .collect()
        }))
    }
}

impl crate::alerts::AlertStates for AlertLedger {
    async fn of(
        &self,
        rule: crate::alerts::RuleId,
    ) -> Result<Vec<crate::alerts::SeriesState>, StoreError> {
        Ok(self.with(|held| {
            held.states
                .iter()
                .filter(|(kept, _)| *kept == rule)
                .map(|(_, state)| state.clone())
                .collect()
        }))
    }

    async fn of_org(
        &self,
        org: TenantId,
    ) -> Result<Vec<(crate::alerts::RuleId, crate::alerts::SeriesState)>, StoreError> {
        Ok(self.with(|held| {
            let mine: Vec<_> = held
                .rules
                .iter()
                .filter(|rule| rule.org == org)
                .map(|rule| rule.id)
                .collect();
            held.states
                .iter()
                .filter(|(rule, _)| mine.contains(rule))
                .cloned()
                .collect()
        }))
    }

    async fn keep(
        &self,
        rule: crate::alerts::RuleId,
        state: &crate::alerts::SeriesState,
    ) -> Result<(), StoreError> {
        self.with(|held| {
            held.states
                .retain(|(kept, series)| !(*kept == rule && series.labels == state.labels));
            held.states.push((rule, state.clone()));
        });
        Ok(())
    }
}

impl crate::alerts::AlertEvents for AlertLedger {
    async fn record(&self, event: &crate::alerts::AlertEvent) -> Result<(), StoreError> {
        self.with(|held| held.events.push(event.clone()));
        Ok(())
    }

    async fn history(
        &self,
        org: TenantId,
        rule: crate::alerts::RuleId,
        from_ms: u64,
        to_ms: u64,
        limit: u32,
    ) -> Result<Vec<crate::alerts::AlertEvent>, StoreError> {
        Ok(self.with(|held| {
            let mut found: Vec<_> = held
                .events
                .iter()
                .filter(|event| {
                    event.org == org
                        && event.rule == rule
                        && event.at_ms >= from_ms
                        && event.at_ms <= to_ms
                })
                .cloned()
                .collect();
            found.reverse();
            found.sort_by_key(|event| core::cmp::Reverse(event.at_ms));
            found.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
            found
        }))
    }

    async fn prune(&self, before_ms: u64) -> Result<u64, StoreError> {
        Ok(self.with(|held| {
            let before = held.events.len();
            held.events.retain(|event| event.at_ms >= before_ms);
            u64::try_from(before - held.events.len()).unwrap_or(u64::MAX)
        }))
    }
}

impl crate::alerts::AlertOutbox for AlertLedger {
    async fn enqueue(&self, notification: &crate::alerts::Notification) -> Result<(), StoreError> {
        self.with(|held| held.outbox.push(notification.clone()));
        Ok(())
    }

    async fn claim(
        &self,
        now_ms: u64,
        limit: u32,
    ) -> Result<Vec<crate::alerts::Claimed>, StoreError> {
        Ok(self.with(|held| {
            let mut claimed = Vec::new();
            for notification in &held.outbox {
                if claimed.len() >= usize::try_from(limit).unwrap_or(usize::MAX) {
                    break;
                }
                if notification.status != crate::alerts::DeliveryStatus::Pending
                    || notification.next_at_ms > now_ms
                {
                    continue;
                }
                let Some(event) = held
                    .events
                    .iter()
                    .find(|event| event.id == notification.event)
                else {
                    continue;
                };
                let Some(rule) = held.rules.iter().find(|rule| rule.id == notification.rule) else {
                    continue;
                };
                let Some((_, channel)) = held
                    .channels
                    .iter()
                    .find(|(_, channel)| channel.id == notification.channel)
                else {
                    continue;
                };
                claimed.push(crate::alerts::Claimed {
                    notification: notification.clone(),
                    dispatch: crate::alerts::Dispatch {
                        rule: rule.id,
                        name: rule.name.as_str().into(),
                        severity: rule.definition.severity(),
                        org: rule.org,
                        from: event.from,
                        to: event.to,
                        value: event.value,
                        labels: event.labels.clone(),
                        at_ms: event.at_ms,
                    },
                    channel: channel.clone(),
                });
            }
            claimed
        }))
    }

    async fn settle(
        &self,
        id: crate::alerts::NotificationId,
        attempts: u32,
        settlement: &crate::alerts::Settlement,
    ) -> Result<(), StoreError> {
        self.with(|held| {
            if let Some(notification) = held.outbox.iter_mut().find(|held| held.id == id) {
                notification.attempt = attempts;
                match settlement {
                    crate::alerts::Settlement::Sent => {
                        notification.status = crate::alerts::DeliveryStatus::Sent;
                        notification.last_error = None;
                    }
                    crate::alerts::Settlement::Retry { next_at_ms, error } => {
                        notification.next_at_ms = *next_at_ms;
                        notification.last_error = Some(error.clone());
                    }
                    crate::alerts::Settlement::Abandoned { error } => {
                        notification.status = crate::alerts::DeliveryStatus::Abandoned;
                        notification.last_error = Some(error.clone());
                    }
                }
            }
        });
        Ok(())
    }
}

impl crate::alerts::AlertChannels for AlertLedger {
    async fn list(&self, org: TenantId) -> Result<Vec<crate::alerts::ChannelView>, StoreError> {
        Ok(self.with(|held| {
            held.channels
                .iter()
                .filter(|(kept, _)| *kept == org)
                .map(|(_, channel)| view_of(channel))
                .collect()
        }))
    }

    async fn armed(
        &self,
        org: TenantId,
        id: crate::alerts::ChannelId,
    ) -> Result<Option<crate::alerts::Channel>, StoreError> {
        Ok(self.with(|held| {
            held.channels
                .iter()
                .find(|(kept, channel)| *kept == org && channel.id == id)
                .map(|(_, channel)| channel.clone())
        }))
    }

    async fn create(
        &self,
        org: TenantId,
        name: &crate::alerts::ChannelName,
        destination: &crate::alerts::Destination,
        secret: Option<&crate::alerts::WebhookSecret>,
        _by: crate::identity::AccountId,
    ) -> Result<Option<crate::alerts::ChannelView>, StoreError> {
        Ok(self.with(|held| {
            if held
                .channels
                .iter()
                .any(|(kept, channel)| *kept == org && channel.name == *name)
            {
                return None;
            }
            let channel = crate::alerts::Channel {
                id: crate::alerts::ChannelId::fresh(),
                name: name.clone(),
                destination: destination.clone(),
                secret: secret.cloned(),
            };
            let view = view_of(&channel);
            held.channels.push((org, channel));
            Some(view)
        }))
    }

    async fn update(
        &self,
        org: TenantId,
        id: crate::alerts::ChannelId,
        name: &crate::alerts::ChannelName,
        destination: &crate::alerts::Destination,
    ) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            held.channels
                .iter_mut()
                .find(|(kept, channel)| *kept == org && channel.id == id)
                .map(|(_, channel)| {
                    channel.name = name.clone();
                    channel.destination = destination.clone();
                })
                .is_some()
        }))
    }

    async fn delete(
        &self,
        org: TenantId,
        id: crate::alerts::ChannelId,
    ) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            let before = held.channels.len();
            held.channels
                .retain(|(kept, channel)| !(*kept == org && channel.id == id));
            held.outbox
                .retain(|notification| notification.channel != id);
            held.channels.len() != before
        }))
    }

    fn keeps_secrets(&self) -> bool {
        self.sealed
    }
}

impl crate::alerts::AlertDuty for AlertLedger {
    async fn lead(&self, duty: crate::alerts::Duty, slot: u64) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            if held.busy {
                return false;
            }
            if held
                .slots
                .iter()
                .any(|(kept, last)| *kept == duty && *last >= slot)
            {
                return false;
            }
            held.slots.retain(|(kept, _)| *kept != duty);
            held.slots.push((duty, slot));
            true
        }))
    }
}

type Sent = Vec<(Box<str>, crate::alerts::Dispatch)>;

#[derive(Debug, Clone, Default)]
pub struct Couriered {
    sent: Arc<Mutex<Sent>>,
    failing: Arc<Mutex<bool>>,
    private: bool,
}

impl Couriered {
    pub fn admitting_private() -> Self {
        Self {
            private: true,
            ..Self::default()
        }
    }

    pub fn fail(&self, failing: bool) {
        *self.failing.lock().expect("the courier lock") = failing;
    }

    pub fn sent(&self) -> Sent {
        self.sent.lock().expect("the courier lock").clone()
    }

    fn note(
        &self,
        to: Box<str>,
        dispatch: &crate::alerts::Dispatch,
    ) -> Result<(), crate::alerts::DeliveryError> {
        if *self.failing.lock().expect("the courier lock") {
            return Err(crate::alerts::DeliveryError::new(
                "the far end answered 500",
            ));
        }
        self.sent
            .lock()
            .expect("the courier lock")
            .push((to, dispatch.clone()));
        Ok(())
    }
}

impl crate::alerts::WebhookSender for Couriered {
    fn admits(&self, url: &crate::alerts::WebhookUrl) -> bool {
        self.private
            || !matches!(
                url.host(),
                "localhost" | "127.0.0.1" | "::1" | "169.254.169.254"
            )
    }

    async fn send(
        &self,
        url: &crate::alerts::WebhookUrl,
        _secret: &crate::alerts::WebhookSecret,
        dispatch: &crate::alerts::Dispatch,
    ) -> Result<(), crate::alerts::DeliveryError> {
        self.note(url.as_str().into(), dispatch)
    }
}

impl crate::alerts::AlertMailer for Couriered {
    async fn send_alert(
        &self,
        to: &crate::alerts::EmailList,
        dispatch: &crate::alerts::Dispatch,
    ) -> Result<(), crate::alerts::DeliveryError> {
        self.note(to.target().into(), dispatch)
    }
}

impl crate::identity::Mailer for Couriered {
    async fn send_password_reset(
        &self,
        _to: &crate::identity::Email,
        _token: &crate::secret::ResetToken,
    ) -> Result<(), crate::identity::ResetError> {
        Ok(())
    }
}
