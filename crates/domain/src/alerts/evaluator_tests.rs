use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use uuid::Uuid;

use super::entities::{AlertEvent, Observation, RuleDraft, Tick};
use super::errors::ProbeError;
use super::evaluator::Evaluator;
use super::ports::{AlertChannels, AlertRules, SignalProbe};
use super::value_objects::{
    AlertSeverity, AlertStatus, ChannelId, ChannelKind, ChannelName, Comparison, Condition,
    Definition, DefinitionParts, Destination, EventId, Labels, MetricSignal, NoDataPolicy,
    RuleName, Signal, TraceMeasure, TraceSignal,
};
use crate::catalogue::ProjectId;
use crate::clock::UnixSeconds;
use crate::explore::Window;
use crate::identity::AccountId;
use crate::metrics::Aggregation;
use crate::telemetry::TenantId;
use crate::testing::AlertLedger;

const START: u64 = 1_700_000_040;

fn org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn a_project() -> ProjectId {
    ProjectId::new(Uuid::from_u128(30))
}

fn by() -> AccountId {
    AccountId::new(Uuid::from_u128(9))
}

#[derive(Default)]
struct Scripted {
    script: Mutex<VecDeque<Result<Vec<Observation>, ProbeError>>>,
    asked: Mutex<Vec<(TenantId, Option<ProjectId>, Window)>>,
}

impl Scripted {
    fn values(values: &[Option<f64>]) -> Self {
        let scripted = Self::default();
        for value in values {
            scripted.then(Ok(vec![Observation {
                labels: Labels::none(),
                value: *value,
            }]));
        }
        scripted
    }

    fn then(&self, next: Result<Vec<Observation>, ProbeError>) {
        self.script.lock().expect("the lock").push_back(next);
    }
}

impl SignalProbe for &Scripted {
    async fn observe(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        _signal: &Signal,
        window: Window,
    ) -> Result<Vec<Observation>, ProbeError> {
        self.asked
            .lock()
            .expect("the lock")
            .push((org, project, window));
        self.script
            .lock()
            .expect("the lock")
            .pop_front()
            .unwrap_or(Ok(Vec::new()))
    }
}

fn evaluator<'a>(
    ledger: &AlertLedger,
    probe: &'a Scripted,
) -> Evaluator<AlertLedger, AlertLedger, AlertLedger, AlertLedger, AlertLedger, &'a Scripted> {
    Evaluator::new(
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        probe,
    )
}

fn parts(signal: Signal, channels: Vec<ChannelId>) -> DefinitionParts {
    DefinitionParts {
        signal,
        condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
        window_secs: 300,
        pending_secs: 120,
        severity: AlertSeverity::Critical,
        no_data: NoDataPolicy::Ok,
        channels,
    }
}

fn checkout() -> Signal {
    Signal::Traces(TraceSignal {
        service: Some("checkout".into()),
        operation: None,
        environment: None,
        measure: TraceMeasure::ErrorRate,
    })
}

async fn a_rule(ledger: &AlertLedger, parts: DefinitionParts, enabled: bool) {
    AlertRules::create(
        ledger,
        org(1),
        &RuleDraft {
            name: RuleName::try_from(format!("rule {}", ledger.rules().len()).as_str())
                .expect("a name"),
            project: Some(a_project()),
            enabled,
            definition: Definition::new(parts).expect("a definition"),
        },
        by(),
    )
    .await
    .expect("kept")
    .expect("fresh");
}

async fn two_channels(ledger: &AlertLedger) -> Vec<ChannelId> {
    let mut held = Vec::new();
    for name in ["ops", "pager"] {
        held.push(
            AlertChannels::create(
                ledger,
                org(1),
                &ChannelName::try_from(name).expect("a name"),
                &Destination::parse(ChannelKind::Email, "ops@example.com").expect("an address"),
                None,
                by(),
            )
            .await
            .expect("kept")
            .expect("fresh")
            .id,
        );
    }
    held
}

fn minute(at: u64) -> UnixSeconds {
    UnixSeconds::new(START + at * 60)
}

#[tokio::test]
async fn a_rule_goes_pending_then_firing_then_ok_with_one_event_per_change() {
    let ledger = AlertLedger::default();
    let channels = two_channels(&ledger).await;
    a_rule(&ledger, parts(checkout(), channels.clone()), true).await;
    let probe = Scripted::values(&[Some(1.0), Some(9.0), Some(9.0), Some(9.0), Some(2.0)]);
    let evaluating = evaluator(&ledger, &probe);

    for at in 0..5 {
        evaluating.tick(minute(at)).await.expect("a tick");
    }

    let changes: Vec<(AlertStatus, AlertStatus)> = ledger
        .events()
        .iter()
        .map(|event| (event.from, event.to))
        .collect();
    assert_eq!(
        changes,
        vec![
            (AlertStatus::Ok, AlertStatus::Pending),
            (AlertStatus::Pending, AlertStatus::Firing),
            (AlertStatus::Firing, AlertStatus::Ok),
        ]
    );
    let notified: Vec<(EventId, ChannelId)> = ledger
        .notifications()
        .iter()
        .map(|notification| (notification.event, notification.channel))
        .collect();
    let events = ledger.events();
    assert_eq!(
        notified,
        vec![
            (events[1].id, channels[0]),
            (events[1].id, channels[1]),
            (events[2].id, channels[0]),
            (events[2].id, channels[1]),
        ],
        "firing and recovery notify each channel once, pending does not"
    );
    assert!(
        ledger
            .notifications()
            .iter()
            .all(|notification| notification.attempt == 0)
    );
}

#[tokio::test]
async fn an_evaluation_reads_its_own_organisation_project_and_window() {
    let ledger = AlertLedger::default();
    a_rule(&ledger, parts(checkout(), Vec::new()), true).await;
    a_rule(&ledger, parts(checkout(), Vec::new()), false).await;
    let probe = Scripted::values(&[Some(1.0)]);

    let tick = evaluator(&ledger, &probe)
        .tick(minute(0))
        .await
        .expect("a tick");

    assert_eq!(
        tick,
        Tick::Evaluated {
            rules: 1,
            transitions: 0,
            failed: 0,
            pruned: 0
        }
    );
    let asked = probe.asked.lock().expect("the lock");
    assert_eq!(asked.len(), 1, "a disabled rule is not evaluated");
    let (seen_org, seen_project, window) = asked[0];
    assert_eq!(seen_org, org(1));
    assert_eq!(seen_project, Some(a_project()));
    assert_eq!(window.to_unix_nanos(), minute(0).get() * 1_000_000_000);
    assert_eq!(
        window.to_unix_nanos() - window.from_unix_nanos(),
        300_000_000_000
    );
}

#[tokio::test]
async fn a_grouped_rule_keeps_one_state_per_label_set() {
    let ledger = AlertLedger::default();
    let grouped = Signal::Metrics(MetricSignal {
        name: "queue.depth".into(),
        aggregation: Aggregation::Max,
        group_by: vec!["queue".into()],
        filter: BTreeMap::new(),
    });
    let mut asked = parts(grouped, Vec::new());
    asked.pending_secs = 0;
    a_rule(&ledger, asked, true).await;
    let labelled = |queue: &str, value: f64| Observation {
        labels: Labels::new(BTreeMap::from([("queue".into(), queue.into())])),
        value: Some(value),
    };
    let probe = Scripted::default();
    probe.then(Ok(vec![labelled("mail", 9.0), labelled("billing", 1.0)]));
    probe.then(Ok(vec![labelled("billing", 7.0)]));

    let evaluating = evaluator(&ledger, &probe);
    evaluating.tick(minute(0)).await.expect("a tick");
    evaluating.tick(minute(1)).await.expect("a tick");

    let mut states: Vec<(String, AlertStatus)> = ledger
        .states()
        .iter()
        .map(|(_, series)| {
            (
                series.labels.pairs()["queue"].to_string(),
                series.state.status,
            )
        })
        .collect();
    states.sort_by(|one, other| one.0.cmp(&other.0));
    assert_eq!(
        states,
        vec![
            ("billing".to_owned(), AlertStatus::Firing),
            ("mail".to_owned(), AlertStatus::Ok),
        ],
        "mail fired then went silent, which this rule reads as ok"
    );
    let moved: Vec<(&str, AlertStatus)> = ledger
        .events()
        .iter()
        .map(|event| {
            (
                match &*event.labels.pairs()["queue"] {
                    "mail" => "mail",
                    _ => "billing",
                },
                event.to,
            )
        })
        .collect();
    assert_eq!(
        moved,
        vec![
            ("mail", AlertStatus::Firing),
            ("billing", AlertStatus::Firing),
            ("mail", AlertStatus::Ok),
        ]
    );
}

#[tokio::test]
async fn a_tick_already_taken_by_another_replica_is_skipped() {
    let ledger = AlertLedger::default();
    a_rule(&ledger, parts(checkout(), Vec::new()), true).await;
    let probe = Scripted::values(&[Some(1.0), Some(1.0)]);
    let evaluating = evaluator(&ledger, &probe);

    ledger.hold_the_lock(true);
    let locked = evaluating.tick(minute(0)).await.expect("a tick");
    ledger.hold_the_lock(false);
    let first = evaluating.tick(minute(0)).await.expect("a tick");
    let same_slot = evaluating
        .tick(UnixSeconds::new(minute(0).get() + 5))
        .await
        .expect("a tick");

    assert_eq!(locked, Tick::Skipped);
    assert!(matches!(first, Tick::Evaluated { rules: 1, .. }));
    assert_eq!(same_slot, Tick::Skipped, "a minute is evaluated once");
    assert_eq!(probe.asked.lock().expect("the lock").len(), 1);
}

#[tokio::test]
async fn a_rule_whose_signal_cannot_be_read_keeps_its_state() {
    let ledger = AlertLedger::default();
    a_rule(&ledger, parts(checkout(), Vec::new()), true).await;
    let probe = Scripted::default();
    probe.then(Err(ProbeError::Unreachable));

    let tick = evaluator(&ledger, &probe)
        .tick(minute(0))
        .await
        .expect("a tick");

    assert!(matches!(tick, Tick::Evaluated { failed: 1, .. }));
    assert!(ledger.states().is_empty());
}

#[tokio::test]
async fn a_tick_forgets_events_older_than_thirty_days() {
    let ledger = AlertLedger::default();
    a_rule(&ledger, parts(checkout(), Vec::new()), true).await;
    let rule = ledger.rules()[0].id;
    let now_ms = minute(0).get() * 1_000;
    for at_ms in [now_ms - 31 * 86_400_000, now_ms - 86_400_000] {
        ledger.put_event(AlertEvent {
            id: EventId::fresh(),
            rule,
            org: org(1),
            labels: Labels::none(),
            from: AlertStatus::Ok,
            to: AlertStatus::Firing,
            value: None,
            at_ms,
        });
    }
    let probe = Scripted::values(&[Some(1.0)]);

    let tick = evaluator(&ledger, &probe)
        .tick(minute(0))
        .await
        .expect("a tick");

    assert!(matches!(tick, Tick::Evaluated { pruned: 1, .. }));
    assert_eq!(ledger.events().len(), 1);
    let _ = AlertRules::enabled(&ledger).await;
}
