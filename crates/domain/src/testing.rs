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
type Paged = Vec<(Box<str>, crate::oncall::PageDispatch)>;
type Lettered = Vec<(Box<str>, crate::status::Letter)>;

#[derive(Debug, Clone, Default)]
pub struct Couriered {
    sent: Arc<Mutex<Sent>>,
    paged: Arc<Mutex<Paged>>,
    lettered: Arc<Mutex<Lettered>>,
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

    pub fn paged(&self) -> Paged {
        self.paged.lock().expect("the courier lock").clone()
    }

    pub fn lettered(&self) -> Lettered {
        self.lettered.lock().expect("the courier lock").clone()
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

    async fn send_page(
        &self,
        to: &crate::identity::Email,
        dispatch: &crate::oncall::PageDispatch,
    ) -> Result<(), crate::alerts::DeliveryError> {
        if *self.failing.lock().expect("the courier lock") {
            return Err(crate::alerts::DeliveryError::new(
                "the mail relay answered 451",
            ));
        }
        self.paged
            .lock()
            .expect("the courier lock")
            .push((to.as_str().into(), dispatch.clone()));
        Ok(())
    }
}

impl crate::status::StatusMailer for Couriered {
    async fn send_status(
        &self,
        to: &crate::identity::Email,
        letter: &crate::status::Letter,
    ) -> Result<(), crate::alerts::DeliveryError> {
        if *self.failing.lock().expect("the courier lock") {
            return Err(crate::alerts::DeliveryError::new(
                "the mail relay answered 451",
            ));
        }
        self.lettered
            .lock()
            .expect("the courier lock")
            .push((to.as_str().into(), letter.clone()));
        Ok(())
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

#[derive(Debug, Default)]
struct Kept {
    incidents: Vec<crate::incidents::Incident>,
    timeline: Vec<crate::incidents::TimelineEntry>,
    attachments: Vec<(TenantId, crate::incidents::Attachment)>,
}

#[derive(Debug, Clone, Default)]
pub struct IncidentLedger {
    kept: Arc<Mutex<Kept>>,
}

impl IncidentLedger {
    fn with<T>(&self, work: impl FnOnce(&mut Kept) -> T) -> T {
        work(&mut self.kept.lock().expect("the incident ledger lock"))
    }

    pub fn incidents(&self) -> Vec<crate::incidents::Incident> {
        self.with(|kept| kept.incidents.clone())
    }

    pub fn timeline(&self) -> Vec<crate::incidents::TimelineEntry> {
        self.with(|kept| kept.timeline.clone())
    }

    pub fn put(&self, incident: crate::incidents::Incident) {
        self.with(|kept| kept.incidents.push(incident));
    }
}

impl crate::incidents::Incidents for IncidentLedger {
    async fn list(
        &self,
        org: TenantId,
        filter: &crate::incidents::IncidentFilter,
    ) -> Result<Vec<crate::incidents::Incident>, StoreError> {
        Ok(self.with(|kept| {
            let mut found: Vec<crate::incidents::Incident> = kept
                .incidents
                .iter()
                .filter(|incident| incident.org == org)
                .filter(|incident| filter.scope.admits(incident.status))
                .filter(|incident| filter.project.is_none() || incident.project == filter.project)
                .cloned()
                .collect();
            found.sort_by_key(|incident| core::cmp::Reverse(incident.opened_ms));
            found
        }))
    }

    async fn get(
        &self,
        org: TenantId,
        id: crate::incidents::IncidentId,
    ) -> Result<Option<crate::incidents::Incident>, StoreError> {
        Ok(self.with(|kept| {
            kept.incidents
                .iter()
                .find(|incident| incident.org == org && incident.id == id)
                .cloned()
        }))
    }

    async fn open(&self, incident: &crate::incidents::Incident) -> Result<(), StoreError> {
        self.with(|kept| kept.incidents.push(incident.clone()));
        Ok(())
    }

    async fn keep(&self, incident: &crate::incidents::Incident) -> Result<bool, StoreError> {
        Ok(self.with(|kept| {
            kept.incidents
                .iter_mut()
                .find(|held| held.org == incident.org && held.id == incident.id)
                .map(|held| *held = incident.clone())
                .is_some()
        }))
    }

    async fn following(
        &self,
        org: TenantId,
        rule: crate::alerts::RuleId,
    ) -> Result<Vec<crate::incidents::IncidentId>, StoreError> {
        Ok(self.with(|kept| {
            kept.incidents
                .iter()
                .filter(|incident| {
                    incident.org == org
                        && incident.opened_from_alert == Some(rule)
                        && incident.status.is_open()
                })
                .map(|incident| incident.id)
                .collect()
        }))
    }
}

impl crate::incidents::IncidentTimeline for IncidentLedger {
    async fn append(&self, entry: &crate::incidents::TimelineEntry) -> Result<(), StoreError> {
        self.with(|kept| kept.timeline.push(entry.clone()));
        Ok(())
    }

    async fn append_once(
        &self,
        entry: &crate::incidents::TimelineEntry,
    ) -> Result<bool, StoreError> {
        Ok(self.with(|kept| {
            let twin = kept.timeline.iter().any(|held| {
                held.incident == entry.incident
                    && matches!(
                        (&held.change, &entry.change),
                        (
                            crate::incidents::Change::Deploy { service, version, environment },
                            crate::incidents::Change::Deploy {
                                service: other_service,
                                version: other_version,
                                environment: other_environment,
                            },
                        ) if service == other_service
                            && version == other_version
                            && environment == other_environment
                    )
            });
            if !twin {
                kept.timeline.push(entry.clone());
            }
            !twin
        }))
    }

    async fn of(
        &self,
        org: TenantId,
        incident: crate::incidents::IncidentId,
    ) -> Result<Vec<crate::incidents::TimelineEntry>, StoreError> {
        Ok(self.with(|kept| {
            kept.timeline
                .iter()
                .filter(|entry| entry.org == org && entry.incident == incident)
                .cloned()
                .collect()
        }))
    }
}

impl crate::incidents::IncidentAttachments for IncidentLedger {
    async fn attach(
        &self,
        org: TenantId,
        attachment: &crate::incidents::Attachment,
    ) -> Result<(), StoreError> {
        self.with(|kept| kept.attachments.push((org, attachment.clone())));
        Ok(())
    }

    async fn of(
        &self,
        org: TenantId,
        incident: crate::incidents::IncidentId,
    ) -> Result<Vec<crate::incidents::Attachment>, StoreError> {
        Ok(self.with(|kept| {
            kept.attachments
                .iter()
                .filter(|(held, attachment)| *held == org && attachment.incident == incident)
                .map(|(_, attachment)| attachment.clone())
                .collect()
        }))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Crew {
    members: Arc<Mutex<Vec<(TenantId, crate::organisation::Member)>>>,
}

impl Crew {
    pub fn seat(&self, org: TenantId, account: crate::identity::AccountId, email: &str) {
        self.seat_as(org, account, email, crate::organisation::OrgRole::Member);
    }

    pub fn seat_as(
        &self,
        org: TenantId,
        account: crate::identity::AccountId,
        email: &str,
        role: crate::organisation::OrgRole,
    ) {
        self.members.lock().expect("the crew lock").push((
            org,
            crate::organisation::Member {
                account,
                email: crate::identity::Email::try_from(email).expect("an address"),
                name: None,
                photo: None,
                role,
                joined_at_unix_nanos: 0,
            },
        ));
    }
}

impl crate::organisation::Roll for Crew {
    async fn of(&self, org: TenantId) -> Result<Vec<crate::organisation::Member>, StoreError> {
        Ok(self
            .members
            .lock()
            .expect("the crew lock")
            .iter()
            .filter(|(held, _)| *held == org)
            .map(|(_, member)| member.clone())
            .collect())
    }

    async fn grant(
        &self,
        _org: TenantId,
        _account: crate::identity::AccountId,
        _role: crate::organisation::OrgRole,
    ) -> Result<Option<crate::organisation::Member>, StoreError> {
        Ok(None)
    }

    async fn remove(
        &self,
        _org: TenantId,
        _account: crate::identity::AccountId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn standing_of(
        &self,
        _email: &crate::identity::Email,
    ) -> Result<Option<crate::organisation::Holder>, StoreError> {
        Ok(None)
    }

    async fn create_locked(
        &self,
        _email: &crate::identity::Email,
    ) -> Result<crate::identity::AccountId, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn join(
        &self,
        _org: TenantId,
        _account: crate::identity::AccountId,
        _role: crate::organisation::OrgRole,
    ) -> Result<Option<crate::organisation::Joined>, StoreError> {
        Ok(None)
    }

    async fn open_way_in(
        &self,
        _account: crate::identity::AccountId,
        _digest: crate::secret::SecretHash,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct Rostering {
    schedules: Vec<crate::oncall::Schedule>,
    overrides: Vec<(TenantId, crate::oncall::Override)>,
    pages: Vec<crate::oncall::Page>,
    notices: Vec<crate::oncall::Notice>,
    addresses: Vec<(crate::identity::AccountId, crate::identity::Email)>,
}

#[derive(Debug, Clone, Default)]
pub struct OncallLedger {
    held: Arc<Mutex<Rostering>>,
}

impl OncallLedger {
    fn with<T>(&self, work: impl FnOnce(&mut Rostering) -> T) -> T {
        work(&mut self.held.lock().expect("the on-call ledger lock"))
    }

    pub fn put(&self, schedule: crate::oncall::Schedule) {
        self.with(|held| held.schedules.push(schedule));
    }

    pub fn address(&self, account: crate::identity::AccountId, email: &str) {
        let email = crate::identity::Email::try_from(email).expect("an address");
        self.with(|held| held.addresses.push((account, email)));
    }

    pub fn schedules(&self) -> Vec<crate::oncall::Schedule> {
        self.with(|held| held.schedules.clone())
    }

    pub fn pages(&self) -> Vec<crate::oncall::Page> {
        self.with(|held| held.pages.clone())
    }

    pub fn notices(&self) -> Vec<crate::oncall::Notice> {
        self.with(|held| held.notices.clone())
    }
}

impl crate::oncall::Schedules for OncallLedger {
    async fn of_team(
        &self,
        org: TenantId,
        team: crate::organisation::TeamId,
    ) -> Result<Vec<crate::oncall::Schedule>, StoreError> {
        Ok(self.with(|held| {
            held.schedules
                .iter()
                .filter(|schedule| schedule.org == org && schedule.team == team)
                .cloned()
                .collect()
        }))
    }

    async fn of_org(&self, org: TenantId) -> Result<Vec<crate::oncall::Schedule>, StoreError> {
        Ok(self.with(|held| {
            held.schedules
                .iter()
                .filter(|schedule| schedule.org == org)
                .cloned()
                .collect()
        }))
    }

    async fn get(
        &self,
        org: TenantId,
        id: crate::oncall::ScheduleId,
    ) -> Result<Option<crate::oncall::Schedule>, StoreError> {
        Ok(self.with(|held| {
            held.schedules
                .iter()
                .find(|schedule| schedule.org == org && schedule.id == id)
                .cloned()
        }))
    }

    async fn create(
        &self,
        org: TenantId,
        team: crate::organisation::TeamId,
        draft: &crate::oncall::ScheduleDraft,
    ) -> Result<Option<crate::oncall::Schedule>, StoreError> {
        Ok(self.with(|held| {
            if held.schedules.iter().any(|schedule| {
                schedule.org == org && schedule.team == team && schedule.name == draft.name
            }) {
                return None;
            }
            let schedule = crate::oncall::Schedule {
                id: crate::oncall::ScheduleId::fresh(),
                org,
                team,
                name: draft.name.clone(),
                timezone: draft.timezone.clone(),
                rotation: draft.rotation.clone(),
                handoff: draft.handoff,
                length: draft.length,
                starts_on: draft.starts_on,
            };
            held.schedules.push(schedule.clone());
            Some(schedule)
        }))
    }

    async fn update(&self, schedule: &crate::oncall::Schedule) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            held.schedules
                .iter_mut()
                .find(|kept| kept.org == schedule.org && kept.id == schedule.id)
                .map(|kept| *kept = schedule.clone())
                .is_some()
        }))
    }

    async fn delete(
        &self,
        org: TenantId,
        id: crate::oncall::ScheduleId,
    ) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            let before = held.schedules.len();
            held.schedules
                .retain(|schedule| !(schedule.org == org && schedule.id == id));
            held.overrides.retain(|(_, cover)| cover.schedule != id);
            held.pages.retain(|page| page.schedule != id);
            held.schedules.len() != before
        }))
    }

    async fn overrides(
        &self,
        org: TenantId,
        schedule: crate::oncall::ScheduleId,
    ) -> Result<Vec<crate::oncall::Override>, StoreError> {
        Ok(self.with(|held| {
            held.overrides
                .iter()
                .filter(|(kept, cover)| *kept == org && cover.schedule == schedule)
                .map(|(_, cover)| *cover)
                .collect()
        }))
    }

    async fn add_override(
        &self,
        org: TenantId,
        cover: &crate::oncall::Override,
    ) -> Result<(), StoreError> {
        self.with(|held| held.overrides.push((org, *cover)));
        Ok(())
    }

    async fn remove_override(
        &self,
        org: TenantId,
        schedule: crate::oncall::ScheduleId,
        id: crate::oncall::OverrideId,
    ) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            let before = held.overrides.len();
            held.overrides.retain(|(kept, cover)| {
                !(*kept == org && cover.schedule == schedule && cover.id == id)
            });
            held.overrides.len() != before
        }))
    }
}

impl crate::oncall::Pages for OncallLedger {
    async fn raise(&self, page: &crate::oncall::Page) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            let origin = page.source.origin();
            if held
                .pages
                .iter()
                .any(|kept| kept.state.is_open() && kept.source.origin() == origin)
            {
                return false;
            }
            held.pages.push(page.clone());
            true
        }))
    }

    async fn get(
        &self,
        org: TenantId,
        id: crate::oncall::PageId,
    ) -> Result<Option<crate::oncall::Page>, StoreError> {
        Ok(self.with(|held| {
            held.pages
                .iter()
                .find(|page| page.org == org && page.id == id)
                .cloned()
        }))
    }

    async fn list(
        &self,
        org: TenantId,
        scope: crate::oncall::PageScope,
    ) -> Result<Vec<crate::oncall::Page>, StoreError> {
        Ok(self.with(|held| {
            let mut found: Vec<_> = held
                .pages
                .iter()
                .filter(|page| page.org == org && scope.admits(page.state))
                .cloned()
                .collect();
            found.sort_by_key(|page| core::cmp::Reverse(page.created_ms));
            found
        }))
    }

    async fn due(&self, now_ms: u64, limit: u32) -> Result<Vec<crate::oncall::Page>, StoreError> {
        Ok(self.with(|held| {
            held.pages
                .iter()
                .filter(|page| {
                    page.state.awaits() && page.escalate_at_ms.is_some_and(|at| at <= now_ms)
                })
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .cloned()
                .collect()
        }))
    }

    async fn keep(&self, page: &crate::oncall::Page) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            held.pages
                .iter_mut()
                .find(|kept| kept.org == page.org && kept.id == page.id)
                .map(|kept| *kept = page.clone())
                .is_some()
        }))
    }

    async fn resolve(
        &self,
        org: TenantId,
        origin: crate::oncall::Origin,
        at_ms: u64,
    ) -> Result<Vec<crate::oncall::Page>, StoreError> {
        Ok(self.with(|held| {
            let mut resolved = Vec::new();
            for page in &mut held.pages {
                if page.org == org && page.state.is_open() && page.source.origin() == origin {
                    page.state = crate::oncall::PageState::Resolved;
                    page.escalate_at_ms = None;
                    page.resolved_ms = Some(at_ms);
                    resolved.push(page.clone());
                }
            }
            resolved
        }))
    }

    async fn enqueue(&self, notice: &crate::oncall::Notice) -> Result<(), StoreError> {
        self.with(|held| {
            if !held.notices.iter().any(|kept| {
                kept.page == notice.page && kept.step == notice.step && kept.to == notice.to
            }) {
                held.notices.push(notice.clone());
            }
        });
        Ok(())
    }

    async fn claim(
        &self,
        now_ms: u64,
        limit: u32,
    ) -> Result<Vec<crate::oncall::ClaimedNotice>, StoreError> {
        Ok(self.with(|held| {
            let mut claimed = Vec::new();
            for notice in &held.notices {
                if claimed.len() >= usize::try_from(limit).unwrap_or(usize::MAX) {
                    break;
                }
                if notice.status != crate::alerts::DeliveryStatus::Pending
                    || notice.next_at_ms > now_ms
                {
                    continue;
                }
                let Some(page) = held.pages.iter().find(|page| page.id == notice.page) else {
                    continue;
                };
                let Some((_, email)) = held
                    .addresses
                    .iter()
                    .find(|(account, _)| *account == notice.to)
                else {
                    continue;
                };
                claimed.push(crate::oncall::ClaimedNotice {
                    notice: notice.clone(),
                    email: email.clone(),
                    dispatch: crate::oncall::PageDispatch {
                        page: page.id,
                        org: page.org,
                        source: page.source,
                        headline: page.headline.clone(),
                        step: notice.step,
                        at_ms: page.created_ms,
                        ack: None,
                    },
                });
            }
            claimed
        }))
    }

    async fn settle(
        &self,
        id: crate::oncall::NoticeId,
        attempts: u32,
        settlement: &crate::alerts::Settlement,
    ) -> Result<(), StoreError> {
        self.with(|held| {
            if let Some(notice) = held.notices.iter_mut().find(|held| held.id == id) {
                notice.attempt = attempts;
                match settlement {
                    crate::alerts::Settlement::Sent => {
                        notice.status = crate::alerts::DeliveryStatus::Sent;
                        notice.last_error = None;
                    }
                    crate::alerts::Settlement::Retry { next_at_ms, error } => {
                        notice.next_at_ms = *next_at_ms;
                        notice.last_error = Some(error.clone());
                    }
                    crate::alerts::Settlement::Abandoned { error } => {
                        notice.status = crate::alerts::DeliveryStatus::Abandoned;
                        notice.last_error = Some(error.clone());
                    }
                }
            }
        });
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Bench {
    teams: Arc<Mutex<Vec<(TenantId, crate::organisation::Team)>>>,
}

impl Bench {
    pub fn team(
        &self,
        org: TenantId,
        team: crate::organisation::TeamId,
        name: &str,
        members: &[crate::identity::AccountId],
    ) {
        self.teams.lock().expect("the bench lock").push((
            org,
            crate::organisation::Team {
                id: team,
                name: name.into(),
                created_at_unix_nanos: 0,
                members: members.to_vec(),
                projects: Vec::new(),
            },
        ));
    }
}

impl crate::organisation::Squads for Bench {
    async fn of(&self, org: TenantId) -> Result<Vec<crate::organisation::Team>, StoreError> {
        Ok(self
            .teams
            .lock()
            .expect("the bench lock")
            .iter()
            .filter(|(held, _)| *held == org)
            .map(|(_, team)| team.clone())
            .collect())
    }

    async fn create(
        &self,
        _org: TenantId,
        _name: &crate::organisation::TeamName,
    ) -> Result<crate::organisation::Named, StoreError> {
        Ok(crate::organisation::Named::Taken)
    }

    async fn rename(
        &self,
        _org: TenantId,
        _team: crate::organisation::TeamId,
        _name: &crate::organisation::TeamName,
    ) -> Result<crate::organisation::Named, StoreError> {
        Ok(crate::organisation::Named::Taken)
    }

    async fn disband(
        &self,
        _org: TenantId,
        _team: crate::organisation::TeamId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn seat(
        &self,
        _org: TenantId,
        _team: crate::organisation::TeamId,
        _members: &[crate::identity::AccountId],
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn own(
        &self,
        _org: TenantId,
        _team: crate::organisation::TeamId,
        _projects: &[crate::catalogue::ProjectId],
    ) -> Result<bool, StoreError> {
        Ok(false)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PostmortemLedger {
    kept: Arc<Mutex<Vec<crate::postmortems::Postmortem>>>,
}

impl PostmortemLedger {
    pub fn postmortems(&self) -> Vec<crate::postmortems::Postmortem> {
        self.kept
            .lock()
            .expect("the postmortem ledger lock")
            .clone()
    }
}

impl crate::postmortems::Postmortems for PostmortemLedger {
    async fn of(
        &self,
        org: TenantId,
        incident: crate::incidents::IncidentId,
    ) -> Result<Option<crate::postmortems::Postmortem>, StoreError> {
        Ok(self
            .kept
            .lock()
            .expect("the postmortem ledger lock")
            .iter()
            .find(|held| held.org == org && held.incident == incident)
            .cloned())
    }

    async fn summaries(
        &self,
        org: TenantId,
    ) -> Result<Vec<crate::postmortems::PostmortemSummary>, StoreError> {
        Ok(self
            .kept
            .lock()
            .expect("the postmortem ledger lock")
            .iter()
            .filter(|held| held.org == org)
            .map(|held| crate::postmortems::PostmortemSummary {
                incident: held.incident,
                status: held.status,
            })
            .collect())
    }

    async fn create(
        &self,
        postmortem: &crate::postmortems::Postmortem,
    ) -> Result<bool, StoreError> {
        let mut kept = self.kept.lock().expect("the postmortem ledger lock");
        if kept.iter().any(|held| held.incident == postmortem.incident) {
            return Ok(false);
        }
        kept.push(postmortem.clone());
        Ok(true)
    }

    async fn keep(&self, postmortem: &crate::postmortems::Postmortem) -> Result<bool, StoreError> {
        let mut kept = self.kept.lock().expect("the postmortem ledger lock");
        Ok(kept
            .iter_mut()
            .find(|held| held.org == postmortem.org && held.id == postmortem.id)
            .map(|held| *held = postmortem.clone())
            .is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedLetter {
    pub id: crate::status::LetterId,
    pub page: crate::status::StatusPageId,
    pub subscriber: crate::status::SubscriberId,
    pub body: crate::status::LetterBody,
    pub attempt: u32,
    pub next_at_ms: u64,
    pub status: crate::alerts::DeliveryStatus,
    pub last_error: Option<Box<str>>,
}

#[derive(Debug, Default)]
struct Board {
    pages: Vec<crate::status::StatusPage>,
    components: Vec<(TenantId, crate::status::Component)>,
    subscribers: Vec<crate::status::Subscriber>,
    letters: Vec<QueuedLetter>,
}

#[derive(Debug, Clone, Default)]
pub struct StatusLedger {
    held: Arc<Mutex<Board>>,
}

impl StatusLedger {
    fn with<T>(&self, work: impl FnOnce(&mut Board) -> T) -> T {
        work(&mut self.held.lock().expect("the status ledger lock"))
    }

    pub fn pages(&self) -> Vec<crate::status::StatusPage> {
        self.with(|held| held.pages.clone())
    }

    pub fn subscribers(&self) -> Vec<crate::status::Subscriber> {
        self.with(|held| held.subscribers.clone())
    }

    pub fn letters(&self) -> Vec<QueuedLetter> {
        self.with(|held| held.letters.clone())
    }

    pub fn put_page(&self, page: crate::status::StatusPage) {
        self.with(|held| held.pages.push(page));
    }

    pub fn put_component(&self, org: TenantId, component: crate::status::Component) {
        self.with(|held| held.components.push((org, component)));
    }

    pub fn put_subscriber(&self, subscriber: crate::status::Subscriber) {
        self.with(|held| held.subscribers.push(subscriber));
    }
}

impl crate::status::StatusPages for StatusLedger {
    async fn list(&self, org: TenantId) -> Result<Vec<crate::status::StatusPage>, StoreError> {
        Ok(self.with(|held| {
            held.pages
                .iter()
                .filter(|page| page.org == org)
                .cloned()
                .collect()
        }))
    }

    async fn get(
        &self,
        org: TenantId,
        id: crate::status::StatusPageId,
    ) -> Result<Option<crate::status::StatusPage>, StoreError> {
        Ok(self.with(|held| {
            held.pages
                .iter()
                .find(|page| page.org == org && page.id == id)
                .cloned()
        }))
    }

    async fn by_slug(
        &self,
        slug: &crate::status::Slug,
    ) -> Result<Option<crate::status::StatusPage>, StoreError> {
        Ok(self.with(|held| held.pages.iter().find(|page| &page.slug == slug).cloned()))
    }

    async fn by_id(
        &self,
        id: crate::status::StatusPageId,
    ) -> Result<Option<crate::status::StatusPage>, StoreError> {
        Ok(self.with(|held| held.pages.iter().find(|page| page.id == id).cloned()))
    }

    async fn create(
        &self,
        page: &crate::status::StatusPage,
    ) -> Result<crate::status::Saved, StoreError> {
        Ok(self.with(|held| {
            if held.pages.iter().any(|kept| kept.slug == page.slug) {
                return crate::status::Saved::SlugTaken;
            }
            held.pages.push(page.clone());
            crate::status::Saved::Done
        }))
    }

    async fn keep(
        &self,
        page: &crate::status::StatusPage,
    ) -> Result<crate::status::Saved, StoreError> {
        Ok(self.with(|held| {
            if held
                .pages
                .iter()
                .any(|kept| kept.slug == page.slug && kept.id != page.id)
            {
                return crate::status::Saved::SlugTaken;
            }
            held.pages
                .iter_mut()
                .find(|kept| kept.org == page.org && kept.id == page.id)
                .map_or(crate::status::Saved::Gone, |kept| {
                    *kept = page.clone();
                    crate::status::Saved::Done
                })
        }))
    }

    async fn delete(
        &self,
        org: TenantId,
        id: crate::status::StatusPageId,
    ) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            let before = held.pages.len();
            held.pages
                .retain(|page| !(page.org == org && page.id == id));
            let gone = held.pages.len() != before;
            if gone {
                held.components
                    .retain(|(_, component)| component.page != id);
                held.subscribers.retain(|subscriber| subscriber.page != id);
                held.letters.retain(|letter| letter.page != id);
            }
            gone
        }))
    }

    async fn components(
        &self,
        org: TenantId,
        page: crate::status::StatusPageId,
    ) -> Result<Vec<crate::status::Component>, StoreError> {
        Ok(self.with(|held| {
            let mut found: Vec<crate::status::Component> = held
                .components
                .iter()
                .filter(|(kept, component)| *kept == org && component.page == page)
                .map(|(_, component)| component.clone())
                .collect();
            found.sort_by_key(|component| (component.position, component.id));
            found
        }))
    }

    async fn add_component(
        &self,
        org: TenantId,
        component: &crate::status::Component,
    ) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            if !held
                .pages
                .iter()
                .any(|page| page.org == org && page.id == component.page)
            {
                return false;
            }
            held.components.push((org, component.clone()));
            true
        }))
    }

    async fn keep_component(
        &self,
        org: TenantId,
        component: &crate::status::Component,
    ) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            held.components
                .iter_mut()
                .find(|(kept, held)| {
                    *kept == org && held.id == component.id && held.page == component.page
                })
                .map(|(_, held)| *held = component.clone())
                .is_some()
        }))
    }

    async fn remove_component(
        &self,
        org: TenantId,
        page: crate::status::StatusPageId,
        id: crate::status::ComponentId,
    ) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            let before = held.components.len();
            held.components.retain(|(kept, component)| {
                !(*kept == org && component.page == page && component.id == id)
            });
            held.components.len() != before
        }))
    }
}

impl crate::status::StatusSubscribers for StatusLedger {
    async fn find(
        &self,
        page: crate::status::StatusPageId,
        email: &crate::identity::Email,
    ) -> Result<Option<crate::status::Subscriber>, StoreError> {
        Ok(self.with(|held| {
            held.subscribers
                .iter()
                .find(|subscriber| subscriber.page == page && &subscriber.email == email)
                .cloned()
        }))
    }

    async fn get(
        &self,
        id: crate::status::SubscriberId,
    ) -> Result<Option<crate::status::Subscriber>, StoreError> {
        Ok(self.with(|held| {
            held.subscribers
                .iter()
                .find(|subscriber| subscriber.id == id)
                .cloned()
        }))
    }

    async fn add(&self, subscriber: &crate::status::Subscriber) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            if held
                .subscribers
                .iter()
                .any(|kept| kept.page == subscriber.page && kept.email == subscriber.email)
            {
                return false;
            }
            held.subscribers.push(subscriber.clone());
            true
        }))
    }

    async fn keep(&self, subscriber: &crate::status::Subscriber) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            held.subscribers
                .iter_mut()
                .find(|kept| kept.id == subscriber.id)
                .map(|kept| *kept = subscriber.clone())
                .is_some()
        }))
    }

    async fn remove(&self, id: crate::status::SubscriberId) -> Result<bool, StoreError> {
        Ok(self.with(|held| {
            let before = held.subscribers.len();
            held.subscribers.retain(|subscriber| subscriber.id != id);
            held.letters.retain(|letter| letter.subscriber != id);
            held.subscribers.len() != before
        }))
    }

    async fn confirmed(&self, page: crate::status::StatusPageId) -> Result<u64, StoreError> {
        Ok(self.with(|held| {
            u64::try_from(
                held.subscribers
                    .iter()
                    .filter(|subscriber| subscriber.page == page && subscriber.confirmed)
                    .count(),
            )
            .unwrap_or(u64::MAX)
        }))
    }
}

impl crate::status::StatusOutbox for StatusLedger {
    async fn announce(
        &self,
        announcement: &crate::status::Announcement,
    ) -> Result<u64, StoreError> {
        Ok(self.with(|held| {
            let pages: Vec<crate::status::StatusPageId> = held
                .pages
                .iter()
                .filter(|page| page.org == announcement.org && page.public)
                .map(|page| page.id)
                .collect();
            let fresh: Vec<QueuedLetter> = held
                .subscribers
                .iter()
                .filter(|subscriber| subscriber.confirmed && pages.contains(&subscriber.page))
                .map(|subscriber| QueuedLetter {
                    id: crate::status::LetterId::fresh(),
                    page: subscriber.page,
                    subscriber: subscriber.id,
                    body: crate::status::LetterBody::Update(crate::status::Bulletin {
                        title: announcement.title.clone(),
                        kind: announcement.kind,
                        status: announcement.status,
                        text: announcement.text.clone(),
                        at_ms: announcement.at_ms,
                    }),
                    attempt: 0,
                    next_at_ms: announcement.at_ms,
                    status: crate::alerts::DeliveryStatus::Pending,
                    last_error: None,
                })
                .collect();
            let count = u64::try_from(fresh.len()).unwrap_or(u64::MAX);
            held.letters.extend(fresh);
            count
        }))
    }

    async fn invite(&self, invite: &crate::status::Invite) -> Result<(), StoreError> {
        self.with(|held| {
            held.letters.push(QueuedLetter {
                id: invite.id,
                page: invite.page,
                subscriber: invite.subscriber,
                body: crate::status::LetterBody::Confirm,
                attempt: 0,
                next_at_ms: invite.at_ms,
                status: crate::alerts::DeliveryStatus::Pending,
                last_error: None,
            });
        });
        Ok(())
    }

    async fn claim(
        &self,
        now_ms: u64,
        limit: u32,
    ) -> Result<Vec<crate::status::ClaimedLetter>, StoreError> {
        Ok(self.with(|held| {
            held.letters
                .iter()
                .filter(|letter| {
                    letter.status == crate::alerts::DeliveryStatus::Pending
                        && letter.next_at_ms <= now_ms
                })
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .filter_map(|letter| {
                    let page = held.pages.iter().find(|page| page.id == letter.page)?;
                    let subscriber = held
                        .subscribers
                        .iter()
                        .find(|subscriber| subscriber.id == letter.subscriber)?;
                    Some(crate::status::ClaimedLetter {
                        id: letter.id,
                        attempt: letter.attempt,
                        page: page.id,
                        page_name: page.name.clone(),
                        slug: page.slug.clone(),
                        subscriber: subscriber.id,
                        email: subscriber.email.clone(),
                        body: letter.body.clone(),
                    })
                })
                .collect()
        }))
    }

    async fn settle(
        &self,
        id: crate::status::LetterId,
        attempts: u32,
        settlement: &crate::alerts::Settlement,
    ) -> Result<(), StoreError> {
        self.with(|held| {
            if let Some(letter) = held.letters.iter_mut().find(|held| held.id == id) {
                letter.attempt = attempts;
                match settlement {
                    crate::alerts::Settlement::Sent => {
                        letter.status = crate::alerts::DeliveryStatus::Sent;
                        letter.last_error = None;
                    }
                    crate::alerts::Settlement::Retry { next_at_ms, error } => {
                        letter.next_at_ms = *next_at_ms;
                        letter.last_error = Some(error.clone());
                    }
                    crate::alerts::Settlement::Abandoned { error } => {
                        letter.status = crate::alerts::DeliveryStatus::Abandoned;
                        letter.last_error = Some(error.clone());
                    }
                }
            }
        });
        Ok(())
    }
}
