use uuid::Uuid;

use super::entities::{Change, Incident, TimelineEntry};
use super::value_objects::{EntryKind, IncidentId, IncidentSeverity, IncidentStatus};
use crate::alerts::{
    AlertDuty, AlertError, Channel, DRAIN_BATCH, DeliveryError, Destination, Drained, Duty,
    Settlement, WebhookSender, settlement_after,
};
use crate::clock::UnixSeconds;
use crate::store::StoreError;
use crate::telemetry::TenantId;

use core::future::Future;

const MILLIS_PER_SECOND: u64 = 1_000;

pub const ANNOUNCED: [EntryKind; 9] = [
    EntryKind::Opened,
    EntryKind::Status,
    EntryKind::Severity,
    EntryKind::Owner,
    EntryKind::Title,
    EntryKind::Public,
    EntryKind::Comment,
    EntryKind::Paged,
    EntryKind::Acknowledged,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentNotice {
    pub org: TenantId,
    pub incident: IncidentId,
    pub title: Box<str>,
    pub severity: IncidentSeverity,
    pub status: IncidentStatus,
    pub kind: EntryKind,
    pub headline: Box<str>,
    pub text: Option<Box<str>>,
    pub at_ms: u64,
}

fn headline(change: &Change) -> Option<(String, Option<String>)> {
    Some(match change {
        Change::Opened {
            title, severity, ..
        } => (
            format!("Incident opened ({})", severity.as_str()),
            Some(title.as_str().to_owned()),
        ),
        Change::Status { from, to } => {
            (format!("Status: {} → {}", from.as_str(), to.as_str()), None)
        }
        Change::Severity { from, to } => (
            format!("Severity: {} → {}", from.as_str(), to.as_str()),
            None,
        ),
        Change::Owner { to: Some(_), .. } => ("An owner took the incident".to_owned(), None),
        Change::Owner { to: None, .. } => ("The incident has no owner".to_owned(), None),
        Change::Title { to, .. } => ("Renamed".to_owned(), Some(to.as_str().to_owned())),
        Change::Public { to: true, .. } => ("Now shown on the status page".to_owned(), None),
        Change::Public { to: false, .. } => ("Hidden from the status page".to_owned(), None),
        Change::Comment { body, .. } => ("New comment".to_owned(), Some(body.as_str().to_owned())),
        Change::Paged { .. } => ("The on-call was paged".to_owned(), None),
        Change::Acknowledged { .. } => ("The page was acknowledged".to_owned(), None),
        Change::Alert { .. } | Change::Attachment(_) | Change::Deploy { .. } => return None,
    })
}

impl IncidentNotice {
    pub fn of(incident: &Incident, entry: &TimelineEntry) -> Option<Self> {
        let (headline, text) = headline(&entry.change)?;
        Some(Self {
            org: incident.org,
            incident: incident.id,
            title: incident.title.as_str().into(),
            severity: incident.severity,
            status: incident.status,
            kind: entry.change.kind(),
            headline: headline.into(),
            text: text.map(Into::into),
            at_ms: entry.at_ms,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedIncidentNotice {
    pub id: Uuid,
    pub attempt: u32,
    pub notice: IncidentNotice,
    pub channel: Channel,
}

pub trait IncidentOutbox {
    fn claim(
        &self,
        now_ms: u64,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<ClaimedIncidentNotice>, StoreError>> + Send;

    fn settle(
        &self,
        id: Uuid,
        attempts: u32,
        settlement: &Settlement,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

#[derive(Debug)]
pub struct Narrator<'a, O, D, W> {
    outbox: O,
    duty: D,
    webhooks: &'a W,
}

impl<'a, O, D, W> Narrator<'a, O, D, W>
where
    O: IncidentOutbox,
    D: AlertDuty,
    W: WebhookSender + Sync,
{
    pub fn new(outbox: O, duty: D, webhooks: &'a W) -> Self {
        Self {
            outbox,
            duty,
            webhooks,
        }
    }

    pub async fn tick(&self, now: UnixSeconds) -> Result<Drained, AlertError> {
        if !self.duty.lead(Duty::Narrate, now.get()).await? {
            return Ok(Drained::Skipped);
        }
        let now_ms = now.get() * MILLIS_PER_SECOND;
        let (mut sent, mut retried, mut abandoned) = (0, 0, 0);
        for claimed in self.outbox.claim(now_ms, DRAIN_BATCH).await? {
            let attempts = claimed.attempt + 1;
            let settlement = match self.send(&claimed).await {
                Ok(()) => Settlement::Sent,
                Err(error) => settlement_after(attempts, now_ms, &error),
            };
            match settlement {
                Settlement::Sent => sent += 1,
                Settlement::Retry { .. } => retried += 1,
                Settlement::Abandoned { .. } => abandoned += 1,
            }
            self.outbox
                .settle(claimed.id, attempts, &settlement)
                .await?;
        }
        Ok(Drained::Delivered {
            sent,
            retried,
            abandoned,
        })
    }

    async fn send(&self, claimed: &ClaimedIncidentNotice) -> Result<(), DeliveryError> {
        let Destination::Chat(chat, url, _) = &claimed.channel.destination else {
            return Err(DeliveryError::new(
                "only a Slack or Discord channel takes incident updates",
            ));
        };
        if !self.webhooks.admits(url) {
            return Err(DeliveryError::new(
                "this deployment does not send webhooks to private addresses",
            ));
        }
        self.webhooks
            .send_incident(*chat, url, &claimed.notice)
            .await
    }
}
