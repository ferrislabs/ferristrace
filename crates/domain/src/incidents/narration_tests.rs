use std::sync::{Arc, Mutex};

use uuid::Uuid;

use super::entities::{Author, Change, Incident, TimelineEntry};
use super::narration::{ClaimedIncidentNotice, IncidentNotice, IncidentOutbox, Narrator};
use super::value_objects::{
    CommentBody, EntryKind, IncidentId, IncidentSeverity, IncidentStatus, IncidentTitle, TimelineId,
};
use crate::alerts::{
    Channel, ChannelId, ChannelKind, ChannelName, Destination, Drained, Settlement,
};
use crate::clock::UnixSeconds;
use crate::store::StoreError;
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, Couriered};

const NOW: u64 = 1_700_000_000;

fn incident() -> Incident {
    Incident {
        id: IncidentId::new(Uuid::from_u128(5)),
        org: TenantId::new(Uuid::from_u128(1)),
        project: None,
        title: IncidentTitle::try_from("Checkout errors").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status: IncidentStatus::Investigating,
        owner: None,
        public: false,
        opened_ms: 1,
        resolved_ms: None,
        opened_from_alert: None,
        paged_schedule: None,
    }
}

fn entry(change: Change) -> TimelineEntry {
    TimelineEntry {
        id: TimelineId::new(Uuid::from_u128(9)),
        incident: IncidentId::new(Uuid::from_u128(5)),
        org: TenantId::new(Uuid::from_u128(1)),
        author: Author::System,
        change,
        at_ms: NOW * 1_000,
    }
}

#[test]
fn a_status_change_reads_as_its_two_ends_and_an_alert_or_deploy_is_not_announced() {
    let status = IncidentNotice::of(
        &incident(),
        &entry(Change::Status {
            from: IncidentStatus::Investigating,
            to: IncidentStatus::Identified,
        }),
    )
    .expect("announced");
    let deploy = IncidentNotice::of(
        &incident(),
        &entry(Change::Deploy {
            service: "checkout".into(),
            version: "1.2".into(),
            environment: "prod".into(),
        }),
    );

    assert_eq!(&*status.headline, "Status: investigating → identified");
    assert_eq!(status.kind, EntryKind::Status);
    assert_eq!(&*status.title, "Checkout errors");
    assert_eq!(deploy, None);
}

#[test]
fn a_comment_carries_its_text_and_an_opening_carries_the_title() {
    let comment = IncidentNotice::of(
        &incident(),
        &entry(Change::Comment {
            body: CommentBody::try_from("rolled back").expect("a body"),
            public: false,
        }),
    )
    .expect("announced");
    let opened = IncidentNotice::of(
        &incident(),
        &entry(Change::Opened {
            title: IncidentTitle::try_from("Checkout errors").expect("a title"),
            severity: IncidentSeverity::Sev2,
            alert: None,
        }),
    )
    .expect("announced");

    assert_eq!(comment.text.as_deref(), Some("rolled back"));
    assert_eq!(&*opened.headline, "Incident opened (sev2)");
}

#[derive(Clone, Default)]
struct Queue {
    due: Arc<Mutex<Vec<ClaimedIncidentNotice>>>,
    settled: Arc<Mutex<Vec<(Uuid, u32, Settlement)>>>,
}

impl IncidentOutbox for Queue {
    async fn claim(
        &self,
        _now_ms: u64,
        _limit: u32,
    ) -> Result<Vec<ClaimedIncidentNotice>, StoreError> {
        Ok(std::mem::take(&mut *self.due.lock().expect("the lock")))
    }

    async fn settle(
        &self,
        id: Uuid,
        attempts: u32,
        settlement: &Settlement,
    ) -> Result<(), StoreError> {
        self.settled
            .lock()
            .expect("the lock")
            .push((id, attempts, settlement.clone()));
        Ok(())
    }
}

fn claimed(kind: ChannelKind, target: &str, attempt: u32) -> ClaimedIncidentNotice {
    ClaimedIncidentNotice {
        id: Uuid::from_u128(77),
        attempt,
        notice: IncidentNotice::of(
            &incident(),
            &entry(Change::Status {
                from: IncidentStatus::Investigating,
                to: IncidentStatus::Resolved,
            }),
        )
        .expect("announced"),
        channel: Channel {
            id: ChannelId::new(Uuid::from_u128(3)),
            name: ChannelName::try_from("team").expect("a name"),
            destination: Destination::parse(kind, target).expect("a target"),
            secret: None,
        },
    }
}

#[tokio::test]
async fn spec_l13_13_an_update_reaches_the_chat_channel_once_and_settles_as_sent() {
    let queue = Queue::default();
    queue.due.lock().expect("the lock").push(claimed(
        ChannelKind::Discord,
        "https://discord.com/api/webhooks/1/abc",
        0,
    ));
    let hooks = Couriered::default();

    let drained = Narrator::new(queue.clone(), AlertLedger::default(), &hooks)
        .tick(UnixSeconds::new(NOW))
        .await
        .expect("a tick");

    assert_eq!(
        drained,
        Drained::Delivered {
            sent: 1,
            retried: 0,
            abandoned: 0
        }
    );
    let announced = hooks.announced();
    assert_eq!(announced.len(), 1);
    assert_eq!(&*announced[0].0, "https://discord.com/api/webhooks/1/abc");
    assert_eq!(
        queue.settled.lock().expect("the lock")[0].2,
        Settlement::Sent
    );
}

#[tokio::test]
async fn an_update_whose_channel_is_no_chat_is_never_sent_and_is_retried_then_abandoned() {
    let queue = Queue::default();
    {
        let mut due = queue.due.lock().expect("the lock");
        due.push(claimed(ChannelKind::Email, "ops@example.com", 0));
        due.push(claimed(ChannelKind::Email, "ops@example.com", 4));
    }
    let hooks = Couriered::default();

    let drained = Narrator::new(queue.clone(), AlertLedger::default(), &hooks)
        .tick(UnixSeconds::new(NOW))
        .await
        .expect("a tick");

    assert_eq!(
        drained,
        Drained::Delivered {
            sent: 0,
            retried: 1,
            abandoned: 1
        }
    );
    assert!(hooks.announced().is_empty());
}

#[tokio::test]
async fn a_replica_that_does_not_lead_posts_nothing() {
    let queue = Queue::default();
    queue.due.lock().expect("the lock").push(claimed(
        ChannelKind::Discord,
        "https://discord.com/api/webhooks/1/abc",
        0,
    ));
    let (hooks, ledger) = (Couriered::default(), AlertLedger::default());
    ledger.hold_the_lock(true);

    let drained = Narrator::new(queue.clone(), ledger, &hooks)
        .tick(UnixSeconds::new(NOW))
        .await
        .expect("a tick");

    assert_eq!(drained, Drained::Skipped);
    assert!(hooks.announced().is_empty());
}
