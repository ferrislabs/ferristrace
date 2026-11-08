use std::collections::HashMap;

use uuid::Uuid;

use super::entities::Incident;
use super::insights::{Insights, Timing};
use super::value_objects::{IncidentId, IncidentSeverity, IncidentStatus, IncidentTitle};
use crate::alerts::RuleId;
use crate::telemetry::TenantId;

const DAY: u64 = 86_400_000;
const MINUTE: u64 = 60_000;
const FROM: u64 = 100 * DAY;
const TO: u64 = 107 * DAY;

fn incident(
    seed: u128,
    severity: IncidentSeverity,
    opened: u64,
    resolved: Option<u64>,
) -> Incident {
    Incident {
        id: IncidentId::new(Uuid::from_u128(seed)),
        org: TenantId::new(Uuid::from_u128(1)),
        project: None,
        title: IncidentTitle::try_from("Down").expect("a title"),
        severity,
        status: if resolved.is_some() {
            IncidentStatus::Resolved
        } else {
            IncidentStatus::Investigating
        },
        owner: None,
        public: false,
        opened_ms: opened,
        resolved_ms: resolved,
        opened_from_alert: None,
        paged_schedule: None,
    }
}

fn by_alert(mut one: Incident, rule: u128) -> Incident {
    one.opened_from_alert = Some(RuleId::new(Uuid::from_u128(rule)));
    one
}

#[test]
fn timing_is_the_mean_and_the_median_of_its_samples() {
    assert_eq!(Timing::of(vec![]), Timing::default());
    let odd = Timing::of(vec![10, 30, 20]);
    let even = Timing::of(vec![10, 20, 30, 100]);

    assert_eq!(
        (odd.samples, odd.mean_ms, odd.median_ms),
        (3, Some(20), Some(20))
    );
    assert_eq!((even.mean_ms, even.median_ms), (Some(40), Some(25)));
}

#[test]
fn spec_l13_14_the_times_to_acknowledge_and_to_resolve_are_read_from_the_incidents() {
    let first = incident(
        1,
        IncidentSeverity::Sev1,
        FROM + DAY,
        Some(FROM + DAY + 60 * MINUTE),
    );
    let second = incident(
        2,
        IncidentSeverity::Sev1,
        FROM + 2 * DAY,
        Some(FROM + 2 * DAY + 20 * MINUTE),
    );
    let open = incident(3, IncidentSeverity::Sev3, FROM + 3 * DAY, None);
    let responses = HashMap::from([
        (first.id, FROM + DAY + 5 * MINUTE),
        (second.id, FROM + 2 * DAY + 15 * MINUTE),
        (open.id, FROM + 3 * DAY + 2 * MINUTE),
    ]);

    let found = Insights::compute(
        &[first, second, open],
        &responses,
        &HashMap::new(),
        FROM,
        TO,
    );

    assert_eq!(found.overall.incidents, 3);
    assert_eq!((found.overall.open, found.overall.resolved), (1, 2));
    assert_eq!(found.overall.acknowledge.samples, 3);
    assert_eq!(found.overall.acknowledge.median_ms, Some(5 * MINUTE));
    assert_eq!(found.overall.resolve.samples, 2);
    assert_eq!(found.overall.resolve.mean_ms, Some(40 * MINUTE));
    assert_eq!(found.overall.resolve.median_ms, Some(40 * MINUTE));
}

#[test]
fn an_incident_outside_the_window_is_left_out_and_severities_group_in_order() {
    let before = incident(1, IncidentSeverity::Sev1, FROM - 1, None);
    let after = incident(2, IncidentSeverity::Sev1, TO, None);
    let low = incident(3, IncidentSeverity::Sev4, FROM + 1, None);
    let high = incident(4, IncidentSeverity::Sev2, FROM + 2, None);

    let found = Insights::compute(
        &[before, after, low, high],
        &HashMap::new(),
        &HashMap::new(),
        FROM,
        TO,
    );

    assert_eq!(found.overall.incidents, 2);
    let severities: Vec<_> = found
        .by_severity
        .iter()
        .map(|(severity, _)| *severity)
        .collect();
    assert_eq!(
        severities,
        vec![IncidentSeverity::Sev2, IncidentSeverity::Sev4]
    );
}

#[test]
fn an_incident_counts_under_every_service_of_its_alert_and_under_none_without_one() {
    let two = by_alert(incident(1, IncidentSeverity::Sev2, FROM + 1, None), 10);
    let one = by_alert(incident(2, IncidentSeverity::Sev2, FROM + 2, None), 11);
    let by_hand = incident(3, IncidentSeverity::Sev2, FROM + 3, None);
    let services = HashMap::from([
        (
            RuleId::new(Uuid::from_u128(10)),
            vec!["checkout".into(), "payments".into()],
        ),
        (RuleId::new(Uuid::from_u128(11)), vec!["checkout".into()]),
    ]);

    let found = Insights::compute(&[two, one, by_hand], &HashMap::new(), &services, FROM, TO);

    let counted: Vec<(Option<&str>, usize)> = found
        .by_service
        .iter()
        .map(|(name, group)| (name.as_deref(), group.incidents))
        .collect();
    assert_eq!(
        counted,
        vec![(Some("checkout"), 2), (None, 1), (Some("payments"), 1)]
    );
    assert_eq!(
        found.overall.incidents, 3,
        "the total counts each incident once"
    );
}

#[test]
fn the_days_of_the_window_are_all_listed_with_their_counts() {
    let a = incident(1, IncidentSeverity::Sev3, FROM + 10, None);
    let b = incident(2, IncidentSeverity::Sev3, FROM + 20, None);
    let c = incident(3, IncidentSeverity::Sev3, FROM + 2 * DAY + 5, None);

    let found = Insights::compute(&[a, b, c], &HashMap::new(), &HashMap::new(), FROM, TO);

    assert_eq!(found.per_day.len(), 7);
    assert_eq!(found.per_day[0], (FROM, 2));
    assert_eq!(found.per_day[1], (FROM + DAY, 0));
    assert_eq!(found.per_day[2], (FROM + 2 * DAY, 1));
}

#[test]
fn an_empty_window_has_no_timing_and_no_groups() {
    let found = Insights::compute(&[], &HashMap::new(), &HashMap::new(), FROM, TO);

    assert_eq!(found.overall.incidents, 0);
    assert_eq!(found.overall.resolve, Timing::default());
    assert!(found.by_severity.is_empty() && found.by_service.is_empty());
}
