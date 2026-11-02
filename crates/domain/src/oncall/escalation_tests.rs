use uuid::Uuid;

use super::entities::{Escalation, Page, PageSource};
use super::escalation::{escalated, next};
use super::value_objects::{ESCALATE_AFTER_SECS, PageId, PageState, ScheduleId};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::incidents::IncidentId;
use crate::telemetry::TenantId;

const START: u64 = 1_700_000_000;

fn person(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn rotation() -> Vec<AccountId> {
    vec![person(1), person(2), person(3)]
}

fn page(target: u128) -> Page {
    Page {
        id: PageId::new(Uuid::from_u128(50)),
        org: TenantId::new(Uuid::from_u128(60)),
        schedule: ScheduleId::new(Uuid::from_u128(70)),
        target: person(target),
        source: PageSource::Incident(IncidentId::new(Uuid::from_u128(80))),
        headline: "Checkout down".into(),
        state: PageState::Triggered,
        step: 0,
        created_ms: START * 1_000,
        escalate_at_ms: Some((START + ESCALATE_AFTER_SECS) * 1_000),
        acknowledged_by: None,
        acknowledged_ms: None,
        resolved_ms: None,
    }
}

fn after(seconds: u64) -> UnixSeconds {
    UnixSeconds::new(START + seconds)
}

#[test]
fn a_page_waits_for_its_escalation_time() {
    assert_eq!(
        next(
            &page(1),
            &rotation(),
            after(ESCALATE_AFTER_SECS - 1),
            ESCALATE_AFTER_SECS
        ),
        None
    );
}

#[test]
fn an_unacknowledged_page_moves_to_the_next_person_in_the_rotation() {
    assert_eq!(
        next(
            &page(1),
            &rotation(),
            after(ESCALATE_AFTER_SECS),
            ESCALATE_AFTER_SECS
        ),
        Some(Escalation::Next {
            target: person(2),
            step: 1,
            escalate_at_ms: (START + 2 * ESCALATE_AFTER_SECS) * 1_000,
        })
    );
}

#[test]
fn the_next_person_after_the_last_is_the_first() {
    assert_eq!(
        next(&page(3), &rotation(), after(ESCALATE_AFTER_SECS), 60),
        Some(Escalation::Next {
            target: person(1),
            step: 1,
            escalate_at_ms: (START + ESCALATE_AFTER_SECS + 60) * 1_000,
        })
    );
}

#[test]
fn an_acknowledged_or_resolved_page_never_escalates() {
    for state in [PageState::Acknowledged, PageState::Resolved] {
        let mut held = page(1);
        held.state = state;

        assert_eq!(
            next(
                &held,
                &rotation(),
                after(10 * ESCALATE_AFTER_SECS),
                ESCALATE_AFTER_SECS
            ),
            None,
            "{state}"
        );
    }
}

#[test]
fn once_the_rotation_is_exhausted_the_administrators_are_paged() {
    let mut held = page(1);
    held.state = PageState::Escalated;
    held.step = 2;
    held.target = person(3);

    assert_eq!(
        next(
            &held,
            &rotation(),
            after(ESCALATE_AFTER_SECS),
            ESCALATE_AFTER_SECS
        ),
        Some(Escalation::Admins { step: 3 })
    );
}

#[test]
fn a_rotation_of_one_goes_straight_to_the_administrators() {
    assert_eq!(
        next(
            &page(1),
            &[person(1)],
            after(ESCALATE_AFTER_SECS),
            ESCALATE_AFTER_SECS
        ),
        Some(Escalation::Admins { step: 1 })
    );
    assert_eq!(
        next(
            &page(1),
            &[],
            after(ESCALATE_AFTER_SECS),
            ESCALATE_AFTER_SECS
        ),
        Some(Escalation::Admins { step: 1 })
    );
}

#[test]
fn an_override_holder_escalates_to_the_head_of_the_rotation() {
    assert_eq!(
        next(
            &page(9),
            &rotation(),
            after(ESCALATE_AFTER_SECS),
            ESCALATE_AFTER_SECS
        ),
        Some(Escalation::Next {
            target: person(1),
            step: 1,
            escalate_at_ms: (START + 2 * ESCALATE_AFTER_SECS) * 1_000,
        })
    );
}

#[test]
fn a_page_walks_the_rotation_then_reaches_the_administrators_and_stops() {
    let mut held = page(1);
    let mut targets = vec![held.target];
    let mut clock = 0;
    let mut admins = 0;
    while clock < 20 * ESCALATE_AFTER_SECS {
        clock += 30;
        if let Some(escalation) = next(&held, &rotation(), after(clock), ESCALATE_AFTER_SECS) {
            if matches!(escalation, Escalation::Admins { .. }) {
                admins += 1;
            }
            held = escalated(&held, &escalation);
            targets.push(held.target);
        }
    }

    assert_eq!(targets, vec![person(1), person(2), person(3), person(3)]);
    assert_eq!(admins, 1, "the administrators are paged once");
    assert_eq!(held.state, PageState::Escalated);
    assert_eq!(held.step, 3);
    assert_eq!(held.escalate_at_ms, None);
}

#[test]
fn escalating_moves_the_target_the_step_and_the_deadline() {
    let moved = escalated(
        &page(1),
        &Escalation::Next {
            target: person(2),
            step: 1,
            escalate_at_ms: 42,
        },
    );

    assert_eq!(moved.target, person(2));
    assert_eq!(moved.step, 1);
    assert_eq!(moved.state, PageState::Escalated);
    assert_eq!(moved.escalate_at_ms, Some(42));
}
