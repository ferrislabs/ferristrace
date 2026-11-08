use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use uuid::Uuid;

use super::ack::AckKey;
use super::entities::{Origin, PageSource, Paged};
use super::fixtures::{ADA, BOB, BOSS, ME, at, crew, ms, oncall, person, weekly};
use super::paging::{Pager, Paging};
use super::value_objects::{ESCALATE_AFTER_SECS, PageState, ScheduleId};
use crate::access::key_ring_tests::{MINE, THEIRS, an_org};
use crate::alerts::{
    AlertRules, AlertSeverity, Comparison, Condition, Culprits, Definition, DefinitionParts,
    DeliveryStatus, Evaluator, Labels, NoDataPolicy, Observation, ProbeError, RuleDraft, RuleName,
    Signal, SignalProbe, TraceMeasure, TraceSignal,
};
use crate::audit::{Action, Actor, TargetKind};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::incidents::{
    Author, Change, Followers, Incident, IncidentId, IncidentSeverity, IncidentStatus,
    IncidentTitle,
};
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, Couriered, Crew, IncidentLedger, Notebook, OncallLedger};

#[derive(Default)]
struct Scripted(Mutex<VecDeque<Vec<Observation>>>);

impl Scripted {
    fn series(ticks: &[&[(&str, f64)]]) -> Self {
        let scripted = Self::default();
        for tick in ticks {
            scripted.0.lock().expect("the lock").push_back(
                tick.iter()
                    .map(|(label, value)| Observation {
                        labels: if label.is_empty() {
                            Labels::none()
                        } else {
                            Labels::new(BTreeMap::from([("queue".into(), (*label).into())]))
                        },
                        value: Some(*value),
                    })
                    .collect(),
            );
        }
        scripted
    }
}

impl SignalProbe for &Scripted {
    async fn observe(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _signal: &Signal,
        _window: Window,
    ) -> Result<Vec<Observation>, ProbeError> {
        Ok(self
            .0
            .lock()
            .expect("the lock")
            .pop_front()
            .unwrap_or_default())
    }

    async fn culprits(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _signal: &Signal,
        _window: Window,
        _labels: &Labels,
    ) -> Result<Culprits, ProbeError> {
        Ok(Culprits::default())
    }
}

struct World {
    alerts: AlertLedger,
    incidents: IncidentLedger,
    oncall: OncallLedger,
    crew: Crew,
    notebook: Notebook,
    mail: Couriered,
    key: AckKey,
}

impl World {
    fn new() -> Self {
        Self {
            alerts: AlertLedger::default(),
            incidents: IncidentLedger::default(),
            oncall: oncall(),
            crew: crew(),
            notebook: Notebook::default(),
            mail: Couriered::default(),
            key: AckKey::derive(&[3_u8; 32]),
        }
    }

    fn paging(&self) -> Paging<OncallLedger, OncallLedger> {
        Paging::new(self.oncall.clone(), self.oncall.clone())
    }

    async fn a_paging_rule(&self, page: Option<ScheduleId>) {
        AlertRules::create(
            &self.alerts,
            an_org(MINE),
            &RuleDraft {
                name: RuleName::try_from("Checkout errors").expect("a name"),
                project: None,
                enabled: true,
                definition: Definition::new(DefinitionParts {
                    signal: Signal::Traces(TraceSignal {
                        service: Some("checkout".into()),
                        operation: None,
                        environment: None,
                        measure: TraceMeasure::ErrorRate,
                        placement: crate::placement::Placement::default(),
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity: AlertSeverity::Critical,
                    no_data: NoDataPolicy::Ok,
                    channels: Vec::new(),
                    page,
                    mentions: Default::default(),
                })
                .expect("a definition"),
            },
            person(ME),
        )
        .await
        .expect("kept")
        .expect("fresh");
    }

    async fn evaluate(&self, probe: &Scripted, at_secs: u64) {
        Evaluator::new(
            self.alerts.clone(),
            self.alerts.clone(),
            self.alerts.clone(),
            self.alerts.clone(),
            self.alerts.clone(),
            probe,
            Followers::new(self.incidents.clone(), self.incidents.clone()),
            self.paging(),
        )
        .tick(at(at_secs))
        .await
        .expect("a tick");
    }

    async fn page_tick(&self, at_secs: u64) -> Paged {
        Pager::new(
            self.paging(),
            self.alerts.clone(),
            self.crew.clone(),
            self.incidents.clone(),
            self.notebook.clone(),
            &self.mail,
            Some(&self.key),
        )
        .tick(at(at_secs))
        .await
        .expect("a page tick")
    }

    fn mailed(&self) -> Vec<(String, u32)> {
        self.mail
            .paged()
            .into_iter()
            .map(|(to, dispatch)| (to.to_string(), dispatch.step))
            .collect()
    }
}

fn primary() -> ScheduleId {
    weekly().id
}

#[tokio::test]
async fn spec_l2_5_a_firing_alert_pages_the_on_call_and_escalates_until_acknowledged() {
    let world = World::new();
    world.a_paging_rule(Some(primary())).await;
    let probe = Scripted::series(&[&[("", 9.0)], &[("", 9.0)], &[("", 9.0)]]);

    world.evaluate(&probe, 0).await;
    let first = world.page_tick(0).await;
    let mut ticks = Vec::new();
    for second in (30..=ESCALATE_AFTER_SECS).step_by(30) {
        ticks.push(world.page_tick(second).await);
    }

    let pages = world.oncall.pages();
    assert_eq!(pages.len(), 1, "one page for one firing");
    let page = &pages[0];
    assert!(matches!(page.source, PageSource::Alert { .. }));
    assert_eq!(page.headline.as_ref(), "Checkout errors");
    assert_eq!(
        first,
        Paged::Worked {
            escalated: 0,
            sent: 1,
            retried: 0,
            abandoned: 0,
        }
    );
    assert_eq!(
        ticks.last(),
        Some(&Paged::Worked {
            escalated: 1,
            sent: 1,
            retried: 0,
            abandoned: 0,
        }),
        "the page escalates once the delay has passed, not before"
    );
    assert_eq!(page.target, person(ADA), "the next person in the rotation");
    assert_eq!(page.state, PageState::Escalated);
    assert_eq!(page.step, 1);
    assert_eq!(
        world.mailed(),
        vec![
            ("me@example.com".to_owned(), 0),
            ("ada@example.com".to_owned(), 1)
        ],
        "one email per step"
    );
}

#[tokio::test]
async fn spec_l2_5_the_page_goes_to_whoever_is_on_call_when_the_alert_fires() {
    let world = World::new();
    world.a_paging_rule(Some(primary())).await;
    let probe = Scripted::series(&[&[("", 9.0)]]);
    let next_week = 7 * 86_400;

    world.evaluate(&probe, next_week).await;

    assert_eq!(world.oncall.pages()[0].target, person(ADA));
    assert_eq!(world.oncall.pages()[0].created_ms, ms(next_week));
}

#[tokio::test]
async fn a_rule_without_a_schedule_pages_nobody() {
    let world = World::new();
    world.a_paging_rule(None).await;
    let probe = Scripted::series(&[&[("", 9.0)]]);

    world.evaluate(&probe, 0).await;

    assert!(world.oncall.pages().is_empty());
}

#[tokio::test]
async fn a_series_firing_while_a_page_is_open_raises_no_second_page() {
    let world = World::new();
    world.a_paging_rule(Some(primary())).await;
    let probe = Scripted::series(&[
        &[("mail", 9.0), ("sms", 1.0)],
        &[("mail", 9.0), ("sms", 9.0)],
    ]);

    world.evaluate(&probe, 0).await;
    world.evaluate(&probe, 60).await;

    assert_eq!(world.oncall.pages().len(), 1);
    assert_eq!(world.oncall.notices().len(), 1, "and no second email");
}

#[tokio::test]
async fn the_alert_recovering_resolves_its_page_and_a_new_firing_pages_again() {
    let world = World::new();
    world.a_paging_rule(Some(primary())).await;
    let probe = Scripted::series(&[&[("", 9.0)], &[("", 1.0)], &[("", 9.0)]]);

    world.evaluate(&probe, 0).await;
    world.evaluate(&probe, 60).await;
    let after_recovery = world.oncall.pages();
    world.page_tick(2 * ESCALATE_AFTER_SECS).await;
    world.evaluate(&probe, 3 * ESCALATE_AFTER_SECS).await;

    assert_eq!(after_recovery.len(), 1);
    assert_eq!(after_recovery[0].state, PageState::Resolved);
    assert_eq!(after_recovery[0].resolved_ms, Some(ms(60)));
    let pages = world.oncall.pages();
    assert_eq!(pages.len(), 2, "a fresh firing after recovery pages again");
    assert_eq!(
        pages[0].state,
        PageState::Resolved,
        "a resolved page never escalates"
    );
    assert_eq!(pages[0].step, 0);
    assert_eq!(pages[1].state, PageState::Triggered);
}

#[tokio::test]
async fn one_series_recovering_while_another_fires_keeps_the_page_open() {
    let world = World::new();
    world.a_paging_rule(Some(primary())).await;
    let probe = Scripted::series(&[
        &[("mail", 9.0), ("sms", 9.0)],
        &[("mail", 1.0), ("sms", 9.0)],
    ]);

    world.evaluate(&probe, 0).await;
    world.evaluate(&probe, 60).await;

    assert_eq!(world.oncall.pages()[0].state, PageState::Triggered);
}

#[tokio::test]
async fn nobody_on_call_means_no_page() {
    let world = World::new();
    world.a_paging_rule(Some(primary())).await;
    let probe = Scripted::series(&[&[("", 9.0)]]);
    let before_the_rotation = 0_u64;

    Evaluator::new(
        world.alerts.clone(),
        world.alerts.clone(),
        world.alerts.clone(),
        world.alerts.clone(),
        world.alerts.clone(),
        &probe,
        Followers::new(world.incidents.clone(), world.incidents.clone()),
        world.paging(),
    )
    .tick(crate::clock::UnixSeconds::new(before_the_rotation + 60))
    .await
    .expect("a tick");

    assert!(world.oncall.pages().is_empty());
}

#[tokio::test]
async fn an_exhausted_rotation_pages_every_administrator_once_and_records_it() {
    let world = World::new();
    world.a_paging_rule(Some(primary())).await;
    let probe = Scripted::series(&[&[("", 9.0)]]);
    world.evaluate(&probe, 0).await;

    for second in (0..=10 * ESCALATE_AFTER_SECS).step_by(30) {
        world.page_tick(second).await;
    }

    let page = &world.oncall.pages()[0];
    assert_eq!(page.step, 3);
    assert_eq!(page.escalate_at_ms, None);
    assert_eq!(page.target, person(BOB));
    assert_eq!(
        world.mailed(),
        vec![
            ("me@example.com".to_owned(), 0),
            ("ada@example.com".to_owned(), 1),
            ("bob@example.com".to_owned(), 2),
            ("boss@example.com".to_owned(), 3),
        ]
    );
    let escalations: Vec<_> = world
        .notebook
        .entries()
        .into_iter()
        .filter(|entry| entry.action == Action::PageEscalated)
        .collect();
    assert_eq!(escalations.len(), 3);
    assert!(escalations.iter().all(|entry| entry.actor == Actor::System
        && entry.target.kind == TargetKind::Page
        && entry.org == an_org(MINE)));
    assert_eq!(
        escalations[2].detail.get("to"),
        Some(Some(person(BOSS).to_string().as_str()))
    );
}

#[tokio::test]
async fn the_mail_carries_a_link_that_acknowledges_as_its_recipient() {
    let world = World::new();
    world.a_paging_rule(Some(primary())).await;
    let probe = Scripted::series(&[&[("", 9.0)]]);
    world.evaluate(&probe, 0).await;

    world.page_tick(0).await;

    let (_, dispatch) = world.mail.paged().remove(0);
    let token = dispatch.ack.expect("a link");
    let claim = world.key.open(&token, at(60)).expect("it opens");
    assert_eq!(claim.account, person(ME));
    assert_eq!(claim.page, world.oncall.pages()[0].id);
    assert_eq!(claim.org, an_org(MINE));
}

#[tokio::test]
async fn a_failed_page_mail_is_retried_then_sent() {
    let world = World::new();
    world.a_paging_rule(Some(primary())).await;
    let probe = Scripted::series(&[&[("", 9.0)]]);
    world.evaluate(&probe, 0).await;
    world.mail.fail(true);

    let failed = world.page_tick(0).await;
    world.mail.fail(false);
    let early = world.page_tick(30).await;
    let retried = world.page_tick(60).await;

    assert!(matches!(failed, Paged::Worked { retried: 1, .. }));
    assert!(matches!(early, Paged::Worked { sent: 0, .. }));
    assert!(matches!(retried, Paged::Worked { sent: 1, .. }));
    assert_eq!(world.oncall.notices()[0].status, DeliveryStatus::Sent);
}

#[tokio::test]
async fn a_slot_is_worked_once_and_a_held_lock_skips_it() {
    let world = World::new();

    let once = world.page_tick(0).await;
    let again = world.page_tick(10).await;
    world.alerts.hold_the_lock(true);
    let held = world.page_tick(60).await;

    assert!(matches!(once, Paged::Worked { .. }));
    assert_eq!(again, Paged::Skipped);
    assert_eq!(held, Paged::Skipped);
}

fn an_incident(org: TenantId) -> Incident {
    Incident {
        id: IncidentId::fresh(),
        org,
        project: None,
        title: IncidentTitle::try_from("Checkout down").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status: IncidentStatus::Investigating,
        owner: None,
        public: false,
        opened_ms: ms(0),
        resolved_ms: None,
        opened_from_alert: None,
        paged_schedule: Some(primary()),
    }
}

#[tokio::test]
async fn an_incident_page_escalates_into_its_timeline() {
    let world = World::new();
    let incident = an_incident(an_org(MINE));
    world.incidents.put(incident.clone());
    world
        .paging()
        .raise(
            an_org(MINE),
            weekly().id,
            PageSource::Incident(incident.id),
            incident.title.as_str(),
            ms(0),
        )
        .await
        .expect("raised");

    world.page_tick(ESCALATE_AFTER_SECS).await;

    let page = world.oncall.pages()[0].clone();
    let told: Vec<_> = world
        .incidents
        .timeline()
        .into_iter()
        .map(|entry| (entry.author, entry.change))
        .collect();
    assert_eq!(
        told,
        vec![(
            Author::System,
            Change::Paged {
                page: page.id,
                step: 1,
                target: Some(person(ADA)),
            }
        )]
    );
}

#[tokio::test]
async fn resolving_pages_touches_only_the_open_pages_of_that_origin_and_organisation() {
    let world = World::new();
    let mine = an_incident(an_org(MINE));
    let paging = world.paging();
    paging
        .raise(
            an_org(MINE),
            weekly().id,
            PageSource::Incident(mine.id),
            "Mine",
            ms(0),
        )
        .await
        .expect("raised");

    let foreign = paging
        .resolve(an_org(THEIRS), Origin::Incident(mine.id), ms(5))
        .await
        .expect("resolved");
    let other = paging
        .resolve(an_org(MINE), Origin::Incident(IncidentId::fresh()), ms(5))
        .await
        .expect("resolved");
    let resolved = paging
        .resolve(an_org(MINE), Origin::Incident(mine.id), ms(5))
        .await
        .expect("resolved");

    assert!(foreign.is_empty());
    assert!(other.is_empty());
    assert_eq!(resolved.len(), 1);
    assert_eq!(world.oncall.pages()[0].state, PageState::Resolved);
}

#[tokio::test]
async fn a_schedule_of_another_organisation_pages_nobody() {
    let world = World::new();

    let raised = world
        .paging()
        .raise(
            an_org(THEIRS),
            weekly().id,
            PageSource::Incident(IncidentId::new(Uuid::from_u128(5))),
            "Theirs",
            ms(0),
        )
        .await
        .expect("asked");

    assert_eq!(raised, None);
}
