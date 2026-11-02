use uuid::Uuid;

use super::entities::{Announcement, Heralded, LetterBody, StatusPage, Subscriber};
use super::herald::Herald;
use super::link::{LinkKey, Purpose};
use super::ports::{StatusOutbox, StatusSubscribers};
use super::value_objects::{
    ANNOUNCE_TICK_SECS, CONFIRM_LIFETIME_SECS, PageName, Slug, StatusPageId, SubscriberId, Theme,
    UNSUBSCRIBE_LIFETIME_SECS, UpdateKind,
};
use crate::alerts::{DeliveryStatus, RETRY_AFTER_SECS};
use crate::clock::UnixSeconds;
use crate::identity::Email;
use crate::incidents::{IncidentId, IncidentStatus, IncidentTitle};
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, Couriered, StatusLedger};

const NOW: u64 = 1_700_000_010;

fn org() -> TenantId {
    TenantId::new(Uuid::from_u128(1))
}

fn key() -> LinkKey {
    LinkKey::derive(&[7; 32])
}

fn a_page(ledger: &StatusLedger) -> StatusPageId {
    let id = StatusPageId::fresh();
    ledger.put_page(StatusPage {
        id,
        org: org(),
        name: PageName::try_from("Acme").expect("a name"),
        slug: Slug::try_from("acme").expect("a slug"),
        public: true,
        hide_from_search: false,
        theme: Theme::default(),
        intro: None,
        created_ms: 1,
        updated_ms: 1,
    });
    id
}

fn a_subscriber(ledger: &StatusLedger, page: StatusPageId, email: &str) -> SubscriberId {
    let id = SubscriberId::fresh();
    ledger.put_subscriber(Subscriber {
        id,
        page,
        email: Email::try_from(email).expect("an address"),
        confirmed: true,
        invited_ms: 1,
    });
    id
}

async fn announce(ledger: &StatusLedger, text: &str) {
    ledger
        .announce(&Announcement {
            org: org(),
            incident: IncidentId::fresh(),
            title: IncidentTitle::try_from("Payments fail").expect("a title"),
            kind: UpdateKind::Comment,
            status: IncidentStatus::Identified,
            text: text.into(),
            at_ms: NOW * 1_000,
        })
        .await
        .expect("announced");
}

#[tokio::test]
async fn each_letter_is_mailed_once_with_signed_links() {
    let ledger = StatusLedger::default();
    let page = a_page(&ledger);
    let ada = a_subscriber(&ledger, page, "ada@example.com");
    let fresh = Subscriber {
        id: SubscriberId::fresh(),
        page,
        email: Email::try_from("new@example.com").expect("an address"),
        confirmed: false,
        invited_ms: 1,
    };
    StatusSubscribers::add(&ledger, &fresh).await.expect("kept");
    ledger
        .invite(&super::entities::Invite {
            id: super::value_objects::LetterId::fresh(),
            page,
            subscriber: fresh.id,
            at_ms: NOW * 1_000,
        })
        .await
        .expect("invited");
    announce(&ledger, "Some payments fail.").await;
    let courier = Couriered::default();
    let duty = AlertLedger::default();
    let held = key();
    let herald = Herald::new(ledger.clone(), duty.clone(), &courier, Some(&held));

    let worked = herald.tick(UnixSeconds::new(NOW)).await.expect("worked");
    let again = herald
        .tick(UnixSeconds::new(NOW + ANNOUNCE_TICK_SECS))
        .await
        .expect("worked");

    assert_eq!(
        worked,
        Heralded::Worked {
            sent: 2,
            retried: 0,
            abandoned: 0
        }
    );
    assert_eq!(
        again,
        Heralded::Worked {
            sent: 0,
            retried: 0,
            abandoned: 0
        }
    );
    let lettered = courier.lettered();
    assert_eq!(lettered.len(), 2);
    let (to_new, invitation) = &lettered[0];
    assert_eq!(&**to_new, "new@example.com");
    assert_eq!(invitation.body, LetterBody::Confirm);
    let confirm = invitation.confirm.as_deref().expect("a confirmation link");
    assert_eq!(
        held.open(confirm, Purpose::Confirm, UnixSeconds::new(NOW))
            .map(|claim| claim.subscriber),
        Ok(fresh.id)
    );
    assert!(
        held.open(
            confirm,
            Purpose::Confirm,
            UnixSeconds::new(NOW + CONFIRM_LIFETIME_SECS)
        )
        .is_err()
    );
    let (to_ada, update) = &lettered[1];
    assert_eq!(&**to_ada, "ada@example.com");
    assert!(update.confirm.is_none());
    assert_eq!(
        held.open(
            &update.unsubscribe,
            Purpose::Unsubscribe,
            UnixSeconds::new(NOW + UNSUBSCRIBE_LIFETIME_SECS - 1)
        )
        .map(|claim| claim.subscriber),
        Ok(ada)
    );
    assert!(
        ledger
            .letters()
            .iter()
            .all(|letter| letter.status == DeliveryStatus::Sent)
    );
}

#[tokio::test]
async fn a_tick_already_taken_by_another_replica_does_nothing() {
    let ledger = StatusLedger::default();
    let page = a_page(&ledger);
    a_subscriber(&ledger, page, "ada@example.com");
    announce(&ledger, "Some payments fail.").await;
    let courier = Couriered::default();
    let duty = AlertLedger::default();
    duty.hold_the_lock(true);
    let held = key();

    let skipped = Herald::new(ledger.clone(), duty, &courier, Some(&held))
        .tick(UnixSeconds::new(NOW))
        .await
        .expect("worked");

    assert_eq!(skipped, Heralded::Skipped);
    assert!(courier.lettered().is_empty());
}

#[tokio::test]
async fn a_failed_letter_is_retried_and_without_a_key_it_is_abandoned() {
    let ledger = StatusLedger::default();
    let page = a_page(&ledger);
    a_subscriber(&ledger, page, "ada@example.com");
    announce(&ledger, "Some payments fail.").await;
    let courier = Couriered::default();
    courier.fail(true);
    let held = key();

    let failed = Herald::new(
        ledger.clone(),
        AlertLedger::default(),
        &courier,
        Some(&held),
    )
    .tick(UnixSeconds::new(NOW))
    .await
    .expect("worked");
    let unkeyed = Herald::new(ledger.clone(), AlertLedger::default(), &courier, None)
        .tick(UnixSeconds::new(NOW + RETRY_AFTER_SECS[0]))
        .await
        .expect("worked");

    assert_eq!(
        failed,
        Heralded::Worked {
            sent: 0,
            retried: 1,
            abandoned: 0
        }
    );
    assert_eq!(
        unkeyed,
        Heralded::Worked {
            sent: 0,
            retried: 0,
            abandoned: 1
        }
    );
    assert_eq!(ledger.letters()[0].status, DeliveryStatus::Abandoned);
}
