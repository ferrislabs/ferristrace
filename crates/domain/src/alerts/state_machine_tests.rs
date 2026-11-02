use super::entities::{RuleState, Transition};
use super::state_machine::next_state;
use super::value_objects::{AlertStatus, Comparison, Condition, NoDataPolicy};

const MINUTE_MS: u64 = 60_000;
const START_MS: u64 = 1_700_000_000_000;

fn above_five() -> Condition {
    Condition::new(Comparison::Above, 5.0).expect("a finite threshold")
}

fn at(minutes: u64) -> u64 {
    START_MS + minutes * MINUTE_MS
}

fn drive(
    observations: &[Option<f64>],
    pending_secs: u64,
    no_data: NoDataPolicy,
) -> Vec<(AlertStatus, Option<Transition>)> {
    let mut held: Option<RuleState> = None;
    observations
        .iter()
        .enumerate()
        .map(|(minute, observed)| {
            let (state, transition) = next_state(
                held.as_ref(),
                *observed,
                above_five(),
                pending_secs,
                no_data,
                at(minute as u64),
            );
            held = Some(state);
            (state.status, transition)
        })
        .collect()
}

fn statuses(driven: &[(AlertStatus, Option<Transition>)]) -> Vec<AlertStatus> {
    driven.iter().map(|(status, _)| *status).collect()
}

fn changes(driven: &[(AlertStatus, Option<Transition>)]) -> Vec<(AlertStatus, AlertStatus)> {
    driven
        .iter()
        .filter_map(|(_, transition)| transition.map(|moved| (moved.from, moved.to)))
        .collect()
}

#[test]
fn a_quiet_first_evaluation_is_ok_and_records_nothing() {
    let driven = drive(&[Some(1.0)], 120, NoDataPolicy::Ok);

    assert_eq!(statuses(&driven), vec![AlertStatus::Ok]);
    assert!(changes(&driven).is_empty());
}

#[test]
fn a_breach_is_pending_until_the_pending_duration_has_passed_then_fires() {
    let driven = drive(
        &[Some(1.0), Some(9.0), Some(9.0), Some(9.0), Some(9.0)],
        120,
        NoDataPolicy::Ok,
    );

    assert_eq!(
        statuses(&driven),
        vec![
            AlertStatus::Ok,
            AlertStatus::Pending,
            AlertStatus::Pending,
            AlertStatus::Firing,
            AlertStatus::Firing,
        ]
    );
    assert_eq!(
        changes(&driven),
        vec![
            (AlertStatus::Ok, AlertStatus::Pending),
            (AlertStatus::Pending, AlertStatus::Firing),
        ]
    );
}

#[test]
fn a_breach_without_a_pending_duration_fires_at_once() {
    let driven = drive(&[Some(1.0), Some(9.0)], 0, NoDataPolicy::Ok);

    assert_eq!(
        changes(&driven),
        vec![(AlertStatus::Ok, AlertStatus::Firing)]
    );
}

#[test]
fn a_pending_breach_that_clears_goes_back_to_ok_without_firing() {
    let driven = drive(&[Some(9.0), Some(1.0)], 120, NoDataPolicy::Ok);

    assert_eq!(
        changes(&driven),
        vec![
            (AlertStatus::Ok, AlertStatus::Pending),
            (AlertStatus::Pending, AlertStatus::Ok),
        ]
    );
}

#[test]
fn a_firing_rule_recovers_when_the_value_is_back_under_the_threshold() {
    let driven = drive(&[Some(9.0), Some(9.0), Some(2.0)], 60, NoDataPolicy::Ok);

    assert_eq!(
        changes(&driven),
        vec![
            (AlertStatus::Ok, AlertStatus::Pending),
            (AlertStatus::Pending, AlertStatus::Firing),
            (AlertStatus::Firing, AlertStatus::Ok),
        ]
    );
}

#[test]
fn the_since_of_a_state_is_when_it_was_entered_and_survives_repeats() {
    let mut held = None;
    let mut seen = Vec::new();
    for (minute, observed) in [Some(9.0), Some(9.0), Some(9.0)].into_iter().enumerate() {
        let (state, _) = next_state(
            held.as_ref(),
            observed,
            above_five(),
            0,
            NoDataPolicy::Ok,
            at(minute as u64),
        );
        seen.push((state.since_ms, state.evaluated_ms, state.value));
        held = Some(state);
    }

    assert_eq!(
        seen,
        vec![
            (at(0), at(0), Some(9.0)),
            (at(0), at(1), Some(9.0)),
            (at(0), at(2), Some(9.0)),
        ]
    );
}

#[test]
fn no_data_follows_the_policy_of_the_rule() {
    let as_ok = drive(&[Some(9.0), None], 0, NoDataPolicy::Ok);
    let as_no_data = drive(&[Some(1.0), None], 0, NoDataPolicy::NoData);
    let as_firing = drive(&[Some(1.0), None], 600, NoDataPolicy::Firing);

    assert_eq!(statuses(&as_ok), vec![AlertStatus::Firing, AlertStatus::Ok]);
    assert_eq!(
        changes(&as_no_data),
        vec![(AlertStatus::Ok, AlertStatus::NoData)]
    );
    assert_eq!(
        changes(&as_firing),
        vec![(AlertStatus::Ok, AlertStatus::Firing)],
        "no data treated as firing does not wait for the pending duration"
    );
}

#[test]
fn data_returning_after_no_data_starts_a_fresh_pending() {
    let driven = drive(&[None, Some(9.0), Some(9.0)], 60, NoDataPolicy::NoData);

    assert_eq!(
        changes(&driven),
        vec![
            (AlertStatus::Ok, AlertStatus::NoData),
            (AlertStatus::NoData, AlertStatus::Pending),
            (AlertStatus::Pending, AlertStatus::Firing),
        ]
    );
}

#[test]
fn a_transition_carries_the_observed_value_and_the_instant() {
    let (_, transition) = next_state(None, Some(7.5), above_five(), 0, NoDataPolicy::Ok, at(3));

    assert_eq!(
        transition,
        Some(Transition {
            from: AlertStatus::Ok,
            to: AlertStatus::Firing,
            value: Some(7.5),
            at_ms: at(3),
        })
    );
}

#[test]
fn every_comparison_reads_as_its_operator() {
    let table = [
        (Comparison::Above, 5.0, false),
        (Comparison::Above, 5.1, true),
        (Comparison::AtLeast, 5.0, true),
        (Comparison::Below, 5.0, false),
        (Comparison::Below, 4.9, true),
        (Comparison::AtMost, 5.0, true),
    ];
    for (op, value, fires) in table {
        let condition = Condition::new(op, 5.0).expect("a threshold");
        let (state, _) = next_state(None, Some(value), condition, 0, NoDataPolicy::Ok, at(0));

        assert_eq!(state.status == AlertStatus::Firing, fires, "{op} {value}");
    }
}

#[test]
fn only_firing_and_recovery_notify() {
    let moved = |from, to| Transition {
        from,
        to,
        value: None,
        at_ms: 0,
    };

    assert!(moved(AlertStatus::Pending, AlertStatus::Firing).notifies());
    assert!(moved(AlertStatus::Ok, AlertStatus::Firing).notifies());
    assert!(moved(AlertStatus::Firing, AlertStatus::Ok).notifies());
    assert!(moved(AlertStatus::Firing, AlertStatus::NoData).notifies());
    assert!(!moved(AlertStatus::Ok, AlertStatus::Pending).notifies());
    assert!(!moved(AlertStatus::Pending, AlertStatus::Ok).notifies());
    assert!(!moved(AlertStatus::Ok, AlertStatus::NoData).notifies());
}
