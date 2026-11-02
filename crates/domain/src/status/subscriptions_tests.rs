use uuid::Uuid;

use super::entities::{LetterBody, LinkNotice, StatusPage};
use super::errors::{LinkRefused, StatusError};
use super::link::{LinkClaim, LinkKey, Purpose};
use super::subscriptions::Subscriptions;
use super::value_objects::{PageName, RESEND_AFTER_SECS, Slug, StatusPageId, SubscriberId, Theme};
use crate::clock::UnixSeconds;
use crate::identity::Email;
use crate::telemetry::TenantId;
use crate::testing::StatusLedger;

const NOW: u64 = 1_700_000_000;

fn key() -> LinkKey {
    LinkKey::derive(&[7; 32])
}

fn email(text: &str) -> Email {
    Email::try_from(text).expect("an address")
}

fn slug(text: &str) -> Slug {
    Slug::try_from(text).expect("a slug")
}

fn at(seconds: u64) -> UnixSeconds {
    UnixSeconds::new(NOW + seconds)
}

fn a_page(ledger: &StatusLedger, text: &str, public: bool) -> StatusPageId {
    let id = StatusPageId::fresh();
    ledger.put_page(StatusPage {
        id,
        org: TenantId::new(Uuid::from_u128(1)),
        name: PageName::try_from("Acme").expect("a name"),
        slug: slug(text),
        public,
        hide_from_search: false,
        theme: Theme::default(),
        intro: None,
        created_ms: 1,
        updated_ms: 1,
    });
    id
}

fn subscriptions<'a>(
    ledger: &StatusLedger,
    key: Option<&'a LinkKey>,
) -> Subscriptions<'a, StatusLedger, StatusLedger, StatusLedger> {
    Subscriptions::new(ledger.clone(), ledger.clone(), ledger.clone(), key)
}

fn signed(
    purpose: Purpose,
    page: StatusPageId,
    subscriber: SubscriberId,
    expires: u64,
) -> Box<str> {
    key().sign(
        &LinkClaim {
            purpose,
            page,
            subscriber,
        },
        UnixSeconds::new(expires),
    )
}

#[tokio::test]
async fn subscribing_keeps_an_unconfirmed_address_and_queues_one_confirmation() {
    let ledger = StatusLedger::default();
    let page = a_page(&ledger, "acme", true);
    let held = key();

    let first = subscriptions(&ledger, Some(&held))
        .subscribe(&slug("acme"), &email("Ada@Example.com"), at(0))
        .await
        .expect("subscribed");
    let again = subscriptions(&ledger, Some(&held))
        .subscribe(&slug("acme"), &email("ada@example.com"), at(60))
        .await
        .expect("subscribed");

    assert!(first);
    assert!(!again, "a second ask within the hour sends nothing more");
    let subscribers = ledger.subscribers();
    assert_eq!(subscribers.len(), 1);
    assert_eq!(subscribers[0].email.as_str(), "ada@example.com");
    assert_eq!(subscribers[0].page, page);
    assert!(!subscribers[0].confirmed);
    let letters = ledger.letters();
    assert_eq!(letters.len(), 1);
    assert_eq!(letters[0].body, LetterBody::Confirm);
}

#[tokio::test]
async fn an_unconfirmed_address_is_asked_again_after_an_hour_and_a_confirmed_one_never() {
    let ledger = StatusLedger::default();
    a_page(&ledger, "acme", true);
    let held = key();
    let desk = subscriptions(&ledger, Some(&held));
    desk.subscribe(&slug("acme"), &email("ada@example.com"), at(0))
        .await
        .expect("subscribed");

    let later = desk
        .subscribe(
            &slug("acme"),
            &email("ada@example.com"),
            at(RESEND_AFTER_SECS),
        )
        .await
        .expect("subscribed");
    let subscriber = ledger.subscribers()[0].id;
    let page = ledger.subscribers()[0].page;
    desk.confirm(
        &signed(Purpose::Confirm, page, subscriber, NOW + 999_999),
        at(RESEND_AFTER_SECS + 1),
    )
    .await
    .expect("confirmed");
    let confirmed = desk
        .subscribe(
            &slug("acme"),
            &email("ada@example.com"),
            at(10 * RESEND_AFTER_SECS),
        )
        .await
        .expect("subscribed");

    assert!(later);
    assert!(!confirmed);
    assert_eq!(ledger.letters().len(), 2);
}

#[tokio::test]
async fn a_private_or_unknown_page_or_a_missing_key_subscribes_nobody() {
    let ledger = StatusLedger::default();
    a_page(&ledger, "hidden", false);
    a_page(&ledger, "acme", true);
    let held = key();

    for (asked, key) in [
        ("hidden", Some(&held)),
        ("nobody", Some(&held)),
        ("acme", None),
    ] {
        assert_eq!(
            subscriptions(&ledger, key)
                .subscribe(&slug(asked), &email("ada@example.com"), at(0))
                .await,
            Ok(false)
        );
    }
    assert!(ledger.subscribers().is_empty());
    assert!(ledger.letters().is_empty());
}

#[tokio::test]
async fn a_confirmation_link_confirms_and_lands_on_the_page() {
    let ledger = StatusLedger::default();
    a_page(&ledger, "acme", true);
    let held = key();
    let desk = subscriptions(&ledger, Some(&held));
    desk.subscribe(&slug("acme"), &email("ada@example.com"), at(0))
        .await
        .expect("subscribed");
    let subscriber = ledger.subscribers()[0].clone();

    let landing = desk
        .confirm(
            &signed(Purpose::Confirm, subscriber.page, subscriber.id, NOW + 10),
            at(5),
        )
        .await
        .expect("confirmed");

    assert_eq!(landing.slug.as_str(), "acme");
    assert_eq!(landing.notice, LinkNotice::Confirmed);
    assert!(ledger.subscribers()[0].confirmed);
}

#[tokio::test]
async fn a_tampered_expired_or_foreign_link_is_refused() {
    let ledger = StatusLedger::default();
    let page = a_page(&ledger, "acme", true);
    let held = key();
    let desk = subscriptions(&ledger, Some(&held));
    desk.subscribe(&slug("acme"), &email("ada@example.com"), at(0))
        .await
        .expect("subscribed");
    let subscriber = ledger.subscribers()[0].id;
    let good = signed(Purpose::Confirm, page, subscriber, NOW + 10);

    assert_eq!(
        desk.confirm(&good, at(10)).await,
        Err(StatusError::Link(LinkRefused::Expired))
    );
    assert_eq!(
        desk.confirm(&good.replace(".c.", ".u."), at(1)).await,
        Err(StatusError::Link(LinkRefused::Tampered))
    );
    assert_eq!(
        desk.unsubscribe(&good, at(1)).await,
        Err(StatusError::Link(LinkRefused::Tampered))
    );
    assert_eq!(
        subscriptions(&ledger, None).confirm(&good, at(1)).await,
        Err(StatusError::Link(LinkRefused::Tampered))
    );
    assert_eq!(
        desk.confirm(
            &signed(Purpose::Confirm, page, SubscriberId::fresh(), NOW + 10),
            at(1)
        )
        .await,
        Err(StatusError::NoSuchSubscriber)
    );
    assert!(!ledger.subscribers()[0].confirmed);
}

#[tokio::test]
async fn an_unsubscribe_link_forgets_the_address_and_works_twice() {
    let ledger = StatusLedger::default();
    let page = a_page(&ledger, "acme", true);
    let held = key();
    let desk = subscriptions(&ledger, Some(&held));
    desk.subscribe(&slug("acme"), &email("ada@example.com"), at(0))
        .await
        .expect("subscribed");
    let subscriber = ledger.subscribers()[0].id;
    let link = signed(Purpose::Unsubscribe, page, subscriber, NOW + 10);

    let once = desk.unsubscribe(&link, at(1)).await.expect("unsubscribed");
    let twice = desk.unsubscribe(&link, at(2)).await.expect("unsubscribed");

    assert_eq!(once.notice, LinkNotice::Unsubscribed);
    assert_eq!(twice.slug.as_str(), "acme");
    assert!(ledger.subscribers().is_empty());
    assert!(
        ledger.letters().is_empty(),
        "its pending letters go with it"
    );
}
