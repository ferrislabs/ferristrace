use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use uuid::Uuid;

use super::culprits::{CulpritPod, Culprits};
use super::entities::{AlertEvent, Observation, Rule, RuleDraft, Tick};
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
use crate::incidents::{
    Author, Change, Followers, Incident, IncidentId, IncidentSeverity, IncidentStatus,
    IncidentTitle,
};
use crate::metrics::Aggregation;
use crate::oncall::Paging;
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, IncidentLedger, OncallLedger};

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

type Named = (TenantId, Option<ProjectId>, Window, Labels);

#[derive(Default)]
struct Scripted {
    script: Mutex<VecDeque<Result<Vec<Observation>, ProbeError>>>,
    asked: Mutex<Vec<(TenantId, Option<ProjectId>, Window)>>,
    culprit: Mutex<Option<Result<Culprits, ProbeError>>>,
    named: Mutex<Vec<Named>>,
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

    async fn culprits(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        _signal: &Signal,
        window: Window,
        labels: &Labels,
    ) -> Result<Culprits, ProbeError> {
        self.named
            .lock()
            .expect("the lock")
            .push((org, project, window, labels.clone()));
        self.culprit
            .lock()
            .expect("the lock")
            .clone()
            .unwrap_or_else(|| Ok(Culprits::default()))
    }
}

type Evaluating<'a> = Evaluator<
    AlertLedger,
    AlertLedger,
    AlertLedger,
    AlertLedger,
    AlertLedger,
    &'a Scripted,
    IncidentLedger,
    IncidentLedger,
    OncallLedger,
    OncallLedger,
>;

fn evaluator<'a>(ledger: &AlertLedger, probe: &'a Scripted) -> Evaluating<'a> {
    following(ledger, probe, &IncidentLedger::default())
}

fn following<'a>(
    ledger: &AlertLedger,
    probe: &'a Scripted,
    incidents: &IncidentLedger,
) -> Evaluating<'a> {
    Evaluator::new(
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        probe,
        Followers::new(incidents.clone(), incidents.clone()),
        Paging::new(OncallLedger::default(), OncallLedger::default()),
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
        page: None,
        mentions: crate::Mentions::default(),
    }
}

fn checkout() -> Signal {
    Signal::Traces(TraceSignal {
        service: Some("checkout".into()),
        operation: None,
        environment: None,
        measure: TraceMeasure::ErrorRate,
        placement: crate::placement::Placement::default(),
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
async fn a_silenced_rule_still_evaluates_and_records_events_but_notifies_nobody() {
    let ledger = AlertLedger::default();
    let channels = two_channels(&ledger).await;
    a_rule(&ledger, parts(checkout(), channels.clone()), true).await;
    let rule = ledger.rules()[0].clone();
    AlertRules::update(
        &ledger,
        &Rule {
            silenced_until_ms: Some(minute(10).get() * 1_000),
            ..rule
        },
    )
    .await
    .expect("silenced");
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
        ],
        "a silenced rule still evaluates and its history still records every change"
    );
    assert!(
        ledger.notifications().is_empty(),
        "a silenced rule notifies nobody"
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
            context: None,
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

fn an_incident(
    rule: super::value_objects::RuleId,
    of: TenantId,
    status: IncidentStatus,
) -> Incident {
    Incident {
        id: IncidentId::fresh(),
        org: of,
        project: None,
        title: IncidentTitle::try_from("Checkout is failing").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status,
        owner: None,
        public: false,
        opened_ms: 1,
        resolved_ms: (status == IncidentStatus::Resolved).then_some(2),
        opened_from_alert: Some(rule),
        paged_schedule: None,
    }
}

#[tokio::test]
async fn a_transition_of_a_linked_rule_appends_a_system_entry_to_its_open_incidents_only() {
    let ledger = AlertLedger::default();
    let mut asked = parts(checkout(), Vec::new());
    asked.pending_secs = 0;
    a_rule(&ledger, asked, true).await;
    let rule = ledger.rules()[0].id;
    let incidents = IncidentLedger::default();
    let open = an_incident(rule, org(1), IncidentStatus::Identified);
    let resolved = an_incident(rule, org(1), IncidentStatus::Resolved);
    let elsewhere = an_incident(rule, org(2), IncidentStatus::Investigating);
    for incident in [&open, &resolved, &elsewhere] {
        incidents.put(incident.clone());
    }
    let probe = Scripted::values(&[Some(9.0), Some(9.0), Some(1.0)]);
    let evaluating = following(&ledger, &probe, &incidents);

    for at in 0..3 {
        evaluating.tick(minute(at)).await.expect("a tick");
    }

    let told: Vec<(IncidentId, Author, Change, u64)> = incidents
        .timeline()
        .into_iter()
        .map(|entry| (entry.incident, entry.author, entry.change, entry.at_ms))
        .collect();
    assert_eq!(
        told,
        vec![
            (
                open.id,
                Author::System,
                Change::Alert {
                    rule,
                    from: AlertStatus::Ok,
                    to: AlertStatus::Firing,
                    value: Some(9.0),
                    labels: Labels::none(),
                    context: None,
                },
                minute(0).get() * 1_000,
            ),
            (
                open.id,
                Author::System,
                Change::Alert {
                    rule,
                    from: AlertStatus::Firing,
                    to: AlertStatus::Ok,
                    value: Some(1.0),
                    labels: Labels::none(),
                    context: None,
                },
                minute(2).get() * 1_000,
            ),
        ],
        "one entry per change, on the open incident of the rule's organisation only"
    );
    assert!(incidents.timeline().iter().all(|entry| entry.org == org(1)));
}

#[tokio::test]
async fn a_flap_that_never_fires_stays_out_of_the_timeline() {
    let ledger = AlertLedger::default();
    let mut asked = parts(checkout(), Vec::new());
    asked.no_data = NoDataPolicy::NoData;
    a_rule(&ledger, asked, true).await;
    let rule = ledger.rules()[0].id;
    let incidents = IncidentLedger::default();
    incidents.put(an_incident(rule, org(1), IncidentStatus::Investigating));
    let probe = Scripted::values(&[
        Some(9.0),
        Some(1.0),
        None,
        Some(1.0),
        Some(9.0),
        Some(9.0),
        Some(9.0),
    ]);
    let evaluating = following(&ledger, &probe, &incidents);

    for at in 0..7 {
        evaluating.tick(minute(at)).await.expect("a tick");
    }

    let moved: Vec<(AlertStatus, AlertStatus)> = ledger
        .events()
        .iter()
        .map(|event| (event.from, event.to))
        .collect();
    assert_eq!(
        moved,
        vec![
            (AlertStatus::Ok, AlertStatus::Pending),
            (AlertStatus::Pending, AlertStatus::Ok),
            (AlertStatus::Ok, AlertStatus::NoData),
            (AlertStatus::NoData, AlertStatus::Ok),
            (AlertStatus::Ok, AlertStatus::Pending),
            (AlertStatus::Pending, AlertStatus::Firing),
        ]
    );
    let told: Vec<Change> = incidents
        .timeline()
        .into_iter()
        .map(|entry| entry.change)
        .collect();
    assert_eq!(
        told,
        vec![Change::Alert {
            rule,
            from: AlertStatus::Pending,
            to: AlertStatus::Firing,
            value: Some(9.0),
            labels: Labels::none(),
            context: None,
        }],
        "ok, pending and no data flaps are history, not incident news"
    );
}

fn blamed() -> Culprits {
    Culprits::new(
        vec![
            CulpritPod {
                name: "checkout-1".into(),
                node: Some("node-a".into()),
                count: 7,
            },
            CulpritPod {
                name: "checkout-2".into(),
                node: Some("node-b".into()),
                count: 3,
            },
        ],
        9,
        vec!["node-a".into(), "node-b".into()],
        vec!["1.4.2".into()],
        &Labels::none(),
    )
}

#[tokio::test]
async fn the_culprits_are_read_only_when_a_rule_goes_firing_and_ride_the_event_and_the_timeline() {
    let ledger = AlertLedger::default();
    let mut asked = parts(checkout(), Vec::new());
    asked.pending_secs = 0;
    a_rule(&ledger, asked, true).await;
    let rule = ledger.rules()[0].id;
    let incidents = IncidentLedger::default();
    incidents.put(an_incident(rule, org(1), IncidentStatus::Investigating));
    let probe = Scripted::values(&[Some(1.0), Some(9.0), Some(9.0), Some(1.0)]);
    *probe.culprit.lock().expect("the lock") = Some(Ok(blamed()));
    let evaluating = following(&ledger, &probe, &incidents);

    for at in 0..4 {
        evaluating.tick(minute(at)).await.expect("a tick");
    }

    let named = probe.named.lock().expect("the lock");
    assert_eq!(named.len(), 1, "once, for the one transition to firing");
    assert_eq!(named[0].0, org(1));
    assert_eq!(named[0].1, Some(a_project()));
    let events = ledger.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].context, Some(blamed()));
    assert_eq!(events[1].context, None, "recovery names nothing");
    let timeline = incidents.timeline();
    assert!(matches!(
        &timeline[0].change,
        Change::Alert { context: Some(context), .. } if *context == blamed()
    ));
    assert!(matches!(
        &timeline[1].change,
        Change::Alert { context: None, .. }
    ));
}

#[tokio::test]
async fn an_unreadable_or_empty_context_leaves_the_firing_event_without_one() {
    for scripted in [Some(Err(ProbeError::Unavailable)), None] {
        let ledger = AlertLedger::default();
        let mut asked = parts(checkout(), Vec::new());
        asked.pending_secs = 0;
        a_rule(&ledger, asked, true).await;
        let probe = Scripted::values(&[Some(9.0)]);
        *probe.culprit.lock().expect("the lock") = scripted;

        evaluator(&ledger, &probe)
            .tick(minute(0))
            .await
            .expect("a tick");

        let events = ledger.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].to, AlertStatus::Firing);
        assert_eq!(events[0].context, None);
    }
}

#[tokio::test]
async fn a_grouped_rule_asks_for_the_culprits_of_the_series_that_fired() {
    let ledger = AlertLedger::default();
    let mut asked = parts(
        Signal::Metrics(MetricSignal {
            name: "queue.depth".into(),
            aggregation: Aggregation::Max,
            group_by: vec!["queue".into()],
            filter: BTreeMap::new(),
        }),
        Vec::new(),
    );
    asked.pending_secs = 0;
    a_rule(&ledger, asked, true).await;
    let probe = Scripted::default();
    let mail = Labels::new(BTreeMap::from([("queue".into(), "mail".into())]));
    probe.then(Ok(vec![
        Observation {
            labels: mail.clone(),
            value: Some(9.0),
        },
        Observation {
            labels: Labels::new(BTreeMap::from([("queue".into(), "sms".into())])),
            value: Some(1.0),
        },
    ]));

    evaluator(&ledger, &probe)
        .tick(minute(0))
        .await
        .expect("a tick");

    let named = probe.named.lock().expect("the lock");
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].3, mail);
}
