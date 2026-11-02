use std::sync::{Arc, Mutex};

use uuid::Uuid;

use super::entities::{Deploy, Deployed};
use super::errors::ReleaseError;
use super::ports::{DeployFeed, DeployMarks};
use super::value_objects::LATE_SECS;
use super::watch::{Followed, Watch};
use crate::access::key_ring_tests::{MINE, OURS, THEIRS, a_project, an_org};
use crate::alerts::{
    AlertRules, AlertSeverity, Comparison, Condition, Definition, DefinitionParts, LogSignal,
    NoDataPolicy, RuleDraft, RuleId, RuleName, Signal, TraceMeasure, TraceSignal,
};
use crate::clock::UnixSeconds;
use crate::explore::Window;
use crate::identity::AccountId;
use crate::incidents::{
    Author, Change, Incident, IncidentId, IncidentSeverity, IncidentStatus, IncidentTimeline,
    IncidentTitle,
};
use crate::placement::Placement;
use crate::store::StoreError;
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, IncidentLedger};

const NOW: u64 = 1_700_000_000;
const MILLIS: u64 = 1_000;
const NANOS: u64 = 1_000_000_000;

#[derive(Default)]
struct Feed {
    deployed: Vec<Deployed>,
    windows: Mutex<Vec<Window>>,
}

impl DeployFeed for Feed {
    async fn deployed(&self, window: Window) -> Result<Vec<Deployed>, StoreError> {
        self.windows.lock().expect("the lock").push(window);
        Ok(self.deployed.clone())
    }
}

#[derive(Debug, Clone, Default)]
struct Marks(Arc<Mutex<Option<u64>>>);

impl Marks {
    fn at(&self) -> Option<u64> {
        *self.0.lock().expect("the lock")
    }

    fn set(&self, to_ms: u64) {
        *self.0.lock().expect("the lock") = Some(to_ms);
    }
}

impl DeployMarks for Marks {
    async fn watermark(&self) -> Result<Option<u64>, StoreError> {
        Ok(self.at())
    }

    async fn advance(&self, to_ms: u64) -> Result<(), StoreError> {
        let mut held = self.0.lock().expect("the lock");
        *held = Some(held.map_or(to_ms, |mark| mark.max(to_ms)));
        Ok(())
    }
}

struct World {
    alerts: AlertLedger,
    incidents: IncidentLedger,
    marks: Marks,
}

impl World {
    fn new() -> Self {
        Self {
            alerts: AlertLedger::default(),
            incidents: IncidentLedger::default(),
            marks: Marks::default(),
        }
    }

    async fn tick(&self, feed: &Feed, now: u64) -> Result<Followed, ReleaseError> {
        Watch::new(
            feed,
            self.incidents.clone(),
            self.incidents.clone(),
            self.alerts.clone(),
            self.marks.clone(),
            self.alerts.clone(),
        )
        .tick(UnixSeconds::new(now))
        .await
    }

    async fn rule(&self, org: TenantId, signal: Signal) -> RuleId {
        AlertRules::create(
            &self.alerts,
            org,
            &RuleDraft {
                name: RuleName::try_from("Checkout errors").expect("a name"),
                project: Some(a_project(OURS)),
                enabled: true,
                definition: Definition::new(DefinitionParts {
                    signal,
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity: AlertSeverity::Critical,
                    no_data: NoDataPolicy::Ok,
                    channels: Vec::new(),
                    page: None,
                })
                .expect("a definition"),
            },
            AccountId::new(Uuid::from_u128(9)),
        )
        .await
        .expect("kept")
        .expect("fresh")
        .id
    }

    fn incident(
        &self,
        org: TenantId,
        opened_ms: u64,
        alert: Option<RuleId>,
        status: IncidentStatus,
    ) -> IncidentId {
        let id = IncidentId::fresh();
        self.incidents.put(Incident {
            id,
            org,
            project: None,
            title: IncidentTitle::try_from("Checkout down").expect("a title"),
            severity: IncidentSeverity::Sev2,
            status,
            owner: None,
            public: false,
            opened_ms,
            resolved_ms: (!status.is_open()).then_some(opened_ms + 1),
            opened_from_alert: alert,
            paged_schedule: None,
        });
        id
    }

    fn deploys_of(&self, incident: IncidentId) -> Vec<(String, String, String, u64)> {
        self.incidents
            .timeline()
            .into_iter()
            .filter(|entry| entry.incident == incident)
            .filter_map(|entry| match entry.change {
                Change::Deploy {
                    service,
                    version,
                    environment,
                } => {
                    assert_eq!(entry.author, Author::System);
                    Some((
                        service.into_string(),
                        version.into_string(),
                        environment.into_string(),
                        entry.at_ms,
                    ))
                }
                _ => None,
            })
            .collect()
    }
}

fn traces_of(service: Option<&str>) -> Signal {
    Signal::Traces(TraceSignal {
        service: service.map(Box::from),
        operation: None,
        environment: None,
        measure: TraceMeasure::ErrorRate,
        placement: Placement::default(),
    })
}

fn deployed(org: TenantId, service: &str, version: &str, at_ms: u64) -> Deployed {
    Deployed {
        org,
        deploy: Deploy {
            service: service.into(),
            version: version.into(),
            environment: "prod".into(),
            at_unix_nanos: at_ms * 1_000_000,
        },
    }
}

fn feed_of(deployed: Vec<Deployed>) -> Feed {
    Feed {
        deployed,
        ..Feed::default()
    }
}

fn now_ms() -> u64 {
    NOW * MILLIS
}

#[tokio::test]
async fn the_first_tick_only_plants_the_watermark() {
    let world = World::new();
    let incident = world.incident(
        an_org(MINE),
        now_ms() - 10_000,
        None,
        IncidentStatus::Investigating,
    );
    let feed = feed_of(vec![deployed(
        an_org(MINE),
        "checkout",
        "2",
        now_ms() - 1_000,
    )]);

    let done = world.tick(&feed, NOW).await;

    assert_eq!(
        done,
        Ok(Followed::Worked {
            deploys: 0,
            entries: 0
        })
    );
    assert_eq!(world.marks.at(), Some(now_ms()));
    assert!(world.deploys_of(incident).is_empty());
    assert!(feed.windows.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn the_feed_is_read_from_the_watermark_minus_the_lateness_to_now() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let feed = Feed::default();

    world.tick(&feed, NOW).await.expect("a tick");

    let windows = feed.windows.lock().expect("the lock").clone();
    assert_eq!(
        windows,
        vec![Window::new((NOW - 60 - LATE_SECS) * NANOS, NOW * NANOS).expect("a window")]
    );
    assert_eq!(world.marks.at(), Some(now_ms()));
}

#[tokio::test]
async fn a_deploy_lands_once_in_an_open_incident_with_no_alert_even_when_seen_again() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let incident = world.incident(
        an_org(MINE),
        now_ms() - 600 * MILLIS,
        None,
        IncidentStatus::Investigating,
    );
    let feed = feed_of(vec![deployed(
        an_org(MINE),
        "checkout",
        "2",
        now_ms() - 30 * MILLIS,
    )]);

    let first = world.tick(&feed, NOW).await;
    let second = world.tick(&feed, NOW + 60).await;

    assert_eq!(
        first,
        Ok(Followed::Worked {
            deploys: 1,
            entries: 1
        })
    );
    assert_eq!(
        second,
        Ok(Followed::Worked {
            deploys: 1,
            entries: 0
        })
    );
    assert_eq!(
        world.deploys_of(incident),
        vec![(
            "checkout".to_owned(),
            "2".to_owned(),
            "prod".to_owned(),
            now_ms() - 30 * MILLIS
        )]
    );
}

#[tokio::test]
async fn an_alert_naming_a_service_follows_only_that_service() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let rule = world.rule(an_org(MINE), traces_of(Some("checkout"))).await;
    let incident = world.incident(
        an_org(MINE),
        now_ms() - 600 * MILLIS,
        Some(rule),
        IncidentStatus::Identified,
    );
    let feed = feed_of(vec![
        deployed(an_org(MINE), "checkout", "2", now_ms() - 30 * MILLIS),
        deployed(an_org(MINE), "mailer", "7", now_ms() - 20 * MILLIS),
    ]);

    world.tick(&feed, NOW).await.expect("a tick");

    let services: Vec<String> = world
        .deploys_of(incident)
        .into_iter()
        .map(|held| held.0)
        .collect();
    assert_eq!(services, vec!["checkout".to_owned()]);
}

#[tokio::test]
async fn a_log_alert_naming_services_follows_those_services() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let rule = world
        .rule(
            an_org(MINE),
            Signal::Logs(LogSignal {
                services: vec!["mailer".into()],
                severities: Vec::new(),
                text: None,
                attributes: Vec::new(),
                placement: Placement::default(),
            }),
        )
        .await;
    let incident = world.incident(
        an_org(MINE),
        now_ms() - 600 * MILLIS,
        Some(rule),
        IncidentStatus::Monitoring,
    );
    let feed = feed_of(vec![
        deployed(an_org(MINE), "checkout", "2", now_ms() - 30 * MILLIS),
        deployed(an_org(MINE), "mailer", "7", now_ms() - 20 * MILLIS),
    ]);

    world.tick(&feed, NOW).await.expect("a tick");

    let services: Vec<String> = world
        .deploys_of(incident)
        .into_iter()
        .map(|held| held.0)
        .collect();
    assert_eq!(services, vec!["mailer".to_owned()]);
}

#[tokio::test]
async fn an_alert_naming_no_service_follows_every_service() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let rule = world.rule(an_org(MINE), traces_of(None)).await;
    let incident = world.incident(
        an_org(MINE),
        now_ms() - 600 * MILLIS,
        Some(rule),
        IncidentStatus::Investigating,
    );
    let feed = feed_of(vec![
        deployed(an_org(MINE), "checkout", "2", now_ms() - 30 * MILLIS),
        deployed(an_org(MINE), "mailer", "7", now_ms() - 20 * MILLIS),
    ]);

    world.tick(&feed, NOW).await.expect("a tick");

    assert_eq!(world.deploys_of(incident).len(), 2);
}

#[tokio::test]
async fn a_deploy_before_the_incident_opened_is_not_in_its_timeline() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let incident = world.incident(
        an_org(MINE),
        now_ms() - 10 * MILLIS,
        None,
        IncidentStatus::Investigating,
    );
    let feed = feed_of(vec![deployed(
        an_org(MINE),
        "checkout",
        "2",
        now_ms() - 30 * MILLIS,
    )]);

    world.tick(&feed, NOW).await.expect("a tick");

    assert!(world.deploys_of(incident).is_empty());
}

#[tokio::test]
async fn a_resolved_incident_is_left_alone() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let incident = world.incident(
        an_org(MINE),
        now_ms() - 600 * MILLIS,
        None,
        IncidentStatus::Resolved,
    );
    let feed = feed_of(vec![deployed(
        an_org(MINE),
        "checkout",
        "2",
        now_ms() - 30 * MILLIS,
    )]);

    world.tick(&feed, NOW).await.expect("a tick");

    assert!(world.deploys_of(incident).is_empty());
}

#[tokio::test]
async fn another_orgs_deploy_never_reaches_this_orgs_incident() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let mine = world.incident(
        an_org(MINE),
        now_ms() - 600 * MILLIS,
        None,
        IncidentStatus::Investigating,
    );
    let theirs = world.incident(
        an_org(THEIRS),
        now_ms() - 600 * MILLIS,
        None,
        IncidentStatus::Investigating,
    );
    let feed = feed_of(vec![deployed(
        an_org(THEIRS),
        "checkout",
        "2",
        now_ms() - 30 * MILLIS,
    )]);

    world.tick(&feed, NOW).await.expect("a tick");

    assert!(world.deploys_of(mine).is_empty());
    assert_eq!(world.deploys_of(theirs).len(), 1);
}

#[tokio::test]
async fn timeline_entries_are_only_ever_appended() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let incident = world.incident(
        an_org(MINE),
        now_ms() - 600 * MILLIS,
        None,
        IncidentStatus::Investigating,
    );
    let feed = feed_of(vec![deployed(
        an_org(MINE),
        "checkout",
        "2",
        now_ms() - 30 * MILLIS,
    )]);
    world.tick(&feed, NOW).await.expect("a tick");
    let before = world.incidents.timeline();

    let later = feed_of(vec![
        deployed(an_org(MINE), "checkout", "2", now_ms() - 30 * MILLIS),
        deployed(an_org(MINE), "checkout", "3", now_ms() + 30 * MILLIS),
    ]);
    world.tick(&later, NOW + 120).await.expect("a tick");

    let after = world.incidents.timeline();
    assert_eq!(&after[..before.len()], &before[..]);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        IncidentTimeline::of(&world.incidents, an_org(MINE), incident)
            .await
            .expect("read")
            .len(),
        2
    );
}

#[tokio::test]
async fn another_replica_holding_the_duty_skips_the_tick() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    world.alerts.hold_the_lock(true);
    let feed = Feed::default();

    let done = world.tick(&feed, NOW).await;

    assert_eq!(done, Ok(Followed::Skipped));
    assert!(feed.windows.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn one_slot_is_worked_once() {
    let world = World::new();
    world.marks.set(now_ms() - 60 * MILLIS);
    let feed = Feed::default();

    let first = world.tick(&feed, NOW).await;
    let again = world.tick(&feed, NOW + 1).await;

    assert!(matches!(first, Ok(Followed::Worked { .. })));
    assert_eq!(again, Ok(Followed::Skipped));
}
