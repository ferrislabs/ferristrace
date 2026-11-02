use uuid::Uuid;

use super::derivation::{by_error_rate, derive, overall, public_updates, redacted, shown};
use super::entities::{Link, Observed, PublicComponent, ServiceLoad};
use super::value_objects::{ComponentName, ComponentStatus, ServiceName, UpdateKind};
use crate::alerts::{AlertSeverity, AlertStatus, Labels, RuleId};
use crate::catalogue::ProjectId;
use crate::identity::AccountId;
use crate::incidents::{
    Attachment, AttachmentId, AttachmentKind, AttachmentLabel, Author, Change, CommentBody,
    Incident, IncidentId, IncidentSeverity, IncidentStatus, IncidentTitle, Reference,
    TimelineEntry, TimelineId,
};
use crate::oncall::PageId;
use crate::telemetry::TenantId;

const DAY_MS: u64 = 86_400_000;
const NOW_MS: u64 = 1_700_000_000_000;

fn rule(seed: u128) -> RuleId {
    RuleId::new(Uuid::from_u128(seed))
}

fn project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn service(name: &str, project: Option<ProjectId>) -> Link {
    Link::Service {
        name: ServiceName::try_from(name).expect("a service"),
        project,
    }
}

fn load(name: &str, project: Option<ProjectId>, requests: u64, errors: u64) -> ServiceLoad {
    ServiceLoad {
        name: name.into(),
        project,
        requests,
        errors,
    }
}

#[test]
fn an_alert_link_follows_whether_its_rule_fires_and_how_badly() {
    let observed = Observed {
        firing: vec![
            (rule(1), AlertSeverity::Critical),
            (rule(2), AlertSeverity::Warning),
            (rule(3), AlertSeverity::Info),
        ],
        services: Vec::new(),
    };
    let table = [
        (rule(1), ComponentStatus::Down),
        (rule(2), ComponentStatus::Degraded),
        (rule(3), ComponentStatus::Degraded),
        (rule(4), ComponentStatus::Operational),
    ];
    for (held, expected) in table {
        assert_eq!(
            derive(&Link::Alert(held), None, &observed),
            expected,
            "{held}"
        );
    }
}

#[test]
fn a_manual_status_never_overrides_an_alert_link() {
    assert_eq!(
        derive(
            &Link::Alert(rule(4)),
            Some(ComponentStatus::Down),
            &Observed::default()
        ),
        ComponentStatus::Operational
    );
}

#[test]
fn a_service_link_follows_its_error_rate_over_the_last_minutes() {
    let table = [
        (0, 0, ComponentStatus::Operational),
        (100, 0, ComponentStatus::Operational),
        (100, 4, ComponentStatus::Operational),
        (100, 5, ComponentStatus::Degraded),
        (100, 24, ComponentStatus::Degraded),
        (100, 25, ComponentStatus::Down),
        (3, 3, ComponentStatus::Down),
        (u64::MAX, u64::MAX, ComponentStatus::Down),
    ];
    for (requests, errors, expected) in table {
        assert_eq!(
            by_error_rate(requests, errors),
            expected,
            "{errors}/{requests}"
        );
        let observed = Observed {
            firing: Vec::new(),
            services: vec![load("checkout", None, requests, errors)],
        };
        assert_eq!(
            derive(&service("checkout", None), None, &observed),
            expected
        );
    }
}

#[test]
fn a_service_link_reads_the_load_of_its_own_project() {
    let observed = Observed {
        firing: Vec::new(),
        services: vec![
            load("checkout", Some(project(1)), 100, 50),
            load("checkout", Some(project(2)), 100, 0),
        ],
    };

    assert_eq!(
        derive(&service("checkout", Some(project(1))), None, &observed),
        ComponentStatus::Down
    );
    assert_eq!(
        derive(&service("checkout", Some(project(2))), None, &observed),
        ComponentStatus::Operational
    );
    assert_eq!(
        derive(&service("checkout", None), None, &observed),
        ComponentStatus::Operational,
        "no load read across the organisation means no evidence"
    );
}

#[test]
fn a_manual_link_shows_its_manual_status_or_operational() {
    for status in ComponentStatus::ALL {
        assert_eq!(
            derive(&Link::Manual, Some(*status), &Observed::default()),
            *status
        );
    }
    assert_eq!(
        derive(&Link::Manual, None, &Observed::default()),
        ComponentStatus::Operational
    );
}

fn component(status: ComponentStatus) -> PublicComponent {
    PublicComponent {
        name: ComponentName::try_from("Checkout").expect("a name"),
        status,
    }
}

#[test]
fn the_overall_status_is_the_worst_component_and_an_open_incident_degrades_it() {
    assert_eq!(overall(&[], false), ComponentStatus::Operational);
    assert_eq!(
        overall(
            &[
                component(ComponentStatus::Degraded),
                component(ComponentStatus::Operational)
            ],
            false
        ),
        ComponentStatus::Degraded
    );
    assert_eq!(
        overall(&[component(ComponentStatus::Down)], true),
        ComponentStatus::Down
    );
    assert_eq!(
        overall(&[component(ComponentStatus::Operational)], true),
        ComponentStatus::Degraded
    );
}

fn incident(public: bool, status: IncidentStatus, resolved_ms: Option<u64>) -> Incident {
    Incident {
        id: IncidentId::fresh(),
        org: TenantId::new(Uuid::from_u128(1)),
        project: None,
        title: IncidentTitle::try_from("Checkout down").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status,
        owner: Some(AccountId::new(Uuid::from_u128(9))),
        public,
        opened_ms: NOW_MS - 30 * DAY_MS,
        resolved_ms,
        opened_from_alert: Some(rule(1)),
        paged_schedule: None,
    }
}

#[test]
fn only_public_incidents_open_or_resolved_within_seven_days_are_shown() {
    let table = [
        (true, IncidentStatus::Investigating, None, true),
        (true, IncidentStatus::Monitoring, None, true),
        (false, IncidentStatus::Investigating, None, false),
        (true, IncidentStatus::Resolved, Some(NOW_MS - DAY_MS), true),
        (
            true,
            IncidentStatus::Resolved,
            Some(NOW_MS - 7 * DAY_MS),
            true,
        ),
        (
            true,
            IncidentStatus::Resolved,
            Some(NOW_MS - 7 * DAY_MS - 1),
            false,
        ),
        (
            false,
            IncidentStatus::Resolved,
            Some(NOW_MS - DAY_MS),
            false,
        ),
    ];
    for (public, status, resolved_ms, expected) in table {
        assert_eq!(
            shown(&incident(public, status, resolved_ms), NOW_MS),
            expected,
            "{public} {status} {resolved_ms:?}"
        );
    }
}

fn entry(change: Change, at_ms: u64) -> TimelineEntry {
    TimelineEntry {
        id: TimelineId::fresh(),
        incident: IncidentId::new(Uuid::from_u128(5)),
        org: TenantId::new(Uuid::from_u128(1)),
        author: Author::Account(AccountId::new(Uuid::from_u128(9))),
        change,
        at_ms,
    }
}

fn comment(text: &str, public: bool) -> Change {
    Change::Comment {
        body: CommentBody::try_from(text).expect("a body"),
        public,
    }
}

#[test]
fn public_updates_hold_status_changes_and_public_comments_only_newest_first() {
    let entries = vec![
        entry(
            Change::Opened {
                title: IncidentTitle::try_from("Checkout down").expect("a title"),
                severity: IncidentSeverity::Sev2,
                alert: Some(rule(1)),
            },
            1,
        ),
        entry(comment("the db at 10.0.0.4 is full", false), 2),
        entry(comment("Some payments fail; we are on it.", true), 3),
        entry(
            Change::Owner {
                from: None,
                to: Some(AccountId::new(Uuid::from_u128(9))),
            },
            4,
        ),
        entry(
            Change::Severity {
                from: IncidentSeverity::Sev2,
                to: IncidentSeverity::Sev1,
            },
            5,
        ),
        entry(
            Change::Alert {
                rule: rule(1),
                from: AlertStatus::Firing,
                to: AlertStatus::Ok,
                value: Some(1.0),
                labels: Labels::none(),
            },
            6,
        ),
        entry(
            Change::Attachment(Attachment {
                id: AttachmentId::fresh(),
                incident: IncidentId::new(Uuid::from_u128(5)),
                reference: Reference::parse(
                    AttachmentKind::Trace,
                    "0af7651916cd43dd8448eb211c80319c",
                )
                .expect("a trace"),
                label: AttachmentLabel::try_from("the slow trace").expect("a label"),
                at_ms: 7,
            }),
            7,
        ),
        entry(
            Change::Paged {
                page: PageId::new(Uuid::from_u128(3)),
                step: 0,
                target: Some(AccountId::new(Uuid::from_u128(9))),
            },
            8,
        ),
        entry(
            Change::Status {
                from: IncidentStatus::Investigating,
                to: IncidentStatus::Identified,
            },
            9,
        ),
        entry(
            Change::Title {
                from: IncidentTitle::try_from("Checkout down").expect("a title"),
                to: IncidentTitle::try_from("Payments fail").expect("a title"),
            },
            10,
        ),
    ];

    let updates = public_updates(&entries);

    let seen: Vec<(u64, UpdateKind, &str)> = updates
        .iter()
        .map(|update| (update.at_ms, update.kind, &*update.text))
        .collect();
    assert_eq!(
        seen,
        vec![
            (9, UpdateKind::Status, "identified"),
            (3, UpdateKind::Comment, "Some payments fail; we are on it."),
            (1, UpdateKind::Opened, "investigating"),
        ]
    );
}

#[test]
fn an_address_in_a_published_text_is_redacted() {
    assert_eq!(
        redacted("Owner: ada@example.com, then (bob.smith+ops@corp.example.org)."),
        "Owner: [redacted], then ([redacted])."
    );
    assert_eq!(
        redacted("no @mention or trailing@ or a@b here"),
        "no @mention or trailing@ or a@b here"
    );
    assert_eq!(redacted("caf\u{e9} ok"), "caf\u{e9} ok");
}
