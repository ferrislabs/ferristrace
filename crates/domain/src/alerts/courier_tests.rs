use uuid::Uuid;

use super::courier::{Courier, RETRY_AFTER_SECS};
use super::entities::{AlertEvent, Drained, Notification, RuleDraft};
use super::ports::{AlertChannels, AlertOutbox, AlertRules};
use super::value_objects::{
    AlertSeverity, AlertStatus, ChannelId, ChannelKind, ChannelName, Comparison, Condition,
    Definition, DefinitionParts, DeliveryStatus, Destination, EventId, Labels, NoDataPolicy,
    NotificationId, RuleName, Signal, TraceMeasure, TraceSignal, WebhookSecret,
};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, Couriered};

const NOW: u64 = 1_700_000_000;

fn org() -> TenantId {
    TenantId::new(Uuid::from_u128(1))
}

fn by() -> AccountId {
    AccountId::new(Uuid::from_u128(9))
}

async fn channel(ledger: &AlertLedger, name: &str, kind: ChannelKind, target: &str) -> ChannelId {
    let secret = WebhookSecret::from_plaintext("s3cret");
    AlertChannels::create(
        ledger,
        org(),
        &ChannelName::try_from(name).expect("a name"),
        &Destination::parse(kind, target).expect("a target"),
        (kind == ChannelKind::Webhook).then_some(&secret),
        by(),
    )
    .await
    .expect("kept")
    .expect("fresh")
    .id
}

async fn a_firing(ledger: &AlertLedger, channels: &[ChannelId]) {
    let rule = AlertRules::create(
        ledger,
        org(),
        &RuleDraft {
            name: RuleName::try_from("Checkout errors").expect("a name"),
            project: None,
            enabled: true,
            definition: Definition::new(DefinitionParts {
                signal: Signal::Traces(TraceSignal {
                    service: None,
                    operation: None,
                    environment: None,
                    measure: TraceMeasure::Count,
                }),
                condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                window_secs: 60,
                pending_secs: 0,
                severity: AlertSeverity::Warning,
                no_data: NoDataPolicy::Ok,
                channels: channels.to_vec(),
                page: None,
            })
            .expect("a definition"),
        },
        by(),
    )
    .await
    .expect("kept")
    .expect("fresh");
    let event = AlertEvent {
        id: EventId::fresh(),
        rule: rule.id,
        org: org(),
        labels: Labels::none(),
        from: AlertStatus::Ok,
        to: AlertStatus::Firing,
        value: Some(9.0),
        at_ms: NOW * 1_000,
    };
    ledger.put_event(event.clone());
    for channel in channels {
        ledger
            .enqueue(&Notification {
                id: NotificationId::fresh(),
                org: org(),
                rule: rule.id,
                event: event.id,
                channel: *channel,
                attempt: 0,
                next_at_ms: NOW * 1_000,
                status: DeliveryStatus::Pending,
                last_error: None,
            })
            .await
            .expect("queued");
    }
}

fn courier<'a>(
    ledger: &AlertLedger,
    mailer: &'a Couriered,
    hooks: &'a Couriered,
) -> Courier<'a, AlertLedger, AlertLedger, Couriered, Couriered> {
    Courier::new(ledger.clone(), ledger.clone(), mailer, hooks)
}

#[tokio::test]
async fn a_change_is_delivered_once_to_each_channel() {
    let ledger = AlertLedger::default();
    let mail = channel(&ledger, "ops", ChannelKind::Email, "ops@example.com").await;
    let hook = channel(
        &ledger,
        "pager",
        ChannelKind::Webhook,
        "https://hooks.example.com/in",
    )
    .await;
    a_firing(&ledger, &[mail, hook]).await;
    let (mailer, hooks) = (Couriered::default(), Couriered::default());

    let first = courier(&ledger, &mailer, &hooks)
        .drain(UnixSeconds::new(NOW))
        .await
        .expect("a drain");
    let second = courier(&ledger, &mailer, &hooks)
        .drain(UnixSeconds::new(NOW + 10))
        .await
        .expect("a drain");

    assert_eq!(
        first,
        Drained::Delivered {
            sent: 2,
            retried: 0,
            abandoned: 0
        }
    );
    assert_eq!(
        second,
        Drained::Delivered {
            sent: 0,
            retried: 0,
            abandoned: 0
        }
    );
    assert_eq!(mailer.sent().len(), 1);
    assert_eq!(hooks.sent().len(), 1);
    assert_eq!(&*hooks.sent()[0].0, "https://hooks.example.com/in");
    assert_eq!(&*hooks.sent()[0].1.name, "Checkout errors");
    assert!(
        ledger
            .notifications()
            .iter()
            .all(|notification| notification.status == DeliveryStatus::Sent
                && notification.attempt == 1)
    );
}

#[tokio::test]
async fn a_failing_webhook_is_retried_at_one_five_fifteen_and_sixty_minutes_then_abandoned() {
    let ledger = AlertLedger::default();
    let hook = channel(
        &ledger,
        "pager",
        ChannelKind::Webhook,
        "https://hooks.example.com/in",
    )
    .await;
    a_firing(&ledger, &[hook]).await;
    let (mailer, hooks) = (Couriered::default(), Couriered::default());
    hooks.fail(true);

    let mut at = NOW;
    let mut waits = Vec::new();
    loop {
        let drained = courier(&ledger, &mailer, &hooks)
            .drain(UnixSeconds::new(at))
            .await
            .expect("a drain");
        let held = ledger.notifications()[0].clone();
        if held.status == DeliveryStatus::Abandoned {
            assert_eq!(
                drained,
                Drained::Delivered {
                    sent: 0,
                    retried: 0,
                    abandoned: 1
                }
            );
            break;
        }
        assert_eq!(
            drained,
            Drained::Delivered {
                sent: 0,
                retried: 1,
                abandoned: 0
            }
        );
        let early = courier(&ledger, &mailer, &hooks)
            .drain(UnixSeconds::new(held.next_at_ms / 1_000 - 1))
            .await
            .expect("a drain");
        assert_eq!(
            early,
            Drained::Delivered {
                sent: 0,
                retried: 0,
                abandoned: 0
            }
        );
        waits.push(held.next_at_ms / 1_000 - at);
        at = held.next_at_ms / 1_000;
    }

    assert_eq!(waits, RETRY_AFTER_SECS.to_vec());
    assert_eq!(RETRY_AFTER_SECS, [60, 300, 900, 3_600]);
    let abandoned = &ledger.notifications()[0];
    assert_eq!(abandoned.attempt, 5);
    assert_eq!(
        abandoned.last_error.as_deref(),
        Some("the far end answered 500")
    );
}

#[tokio::test]
async fn a_drain_held_by_another_replica_is_skipped() {
    let ledger = AlertLedger::default();
    let (mailer, hooks) = (Couriered::default(), Couriered::default());
    ledger.hold_the_lock(true);

    let drained = courier(&ledger, &mailer, &hooks)
        .drain(UnixSeconds::new(NOW))
        .await
        .expect("a drain");

    assert_eq!(drained, Drained::Skipped);
}

#[tokio::test]
async fn a_webhook_to_a_private_address_is_never_sent() {
    let ledger = AlertLedger::default();
    let hook = channel(
        &ledger,
        "pager",
        ChannelKind::Webhook,
        "http://169.254.169.254/latest",
    )
    .await;
    a_firing(&ledger, &[hook]).await;
    let (mailer, hooks) = (Couriered::default(), Couriered::default());

    courier(&ledger, &mailer, &hooks)
        .drain(UnixSeconds::new(NOW))
        .await
        .expect("a drain");

    assert!(hooks.sent().is_empty());
    assert_eq!(ledger.notifications()[0].attempt, 1);
    let _ = AlertRules::list(&ledger, org()).await;
}
