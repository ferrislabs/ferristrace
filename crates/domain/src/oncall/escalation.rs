use super::entities::{Escalation, Page};
use super::value_objects::PageState;
use crate::clock::UnixSeconds;
use crate::identity::AccountId;

const MILLIS_PER_SECOND: u64 = 1_000;

pub fn next(
    page: &Page,
    rotation: &[AccountId],
    now: UnixSeconds,
    escalate_after_secs: u64,
) -> Option<Escalation> {
    if !page.state.awaits() {
        return None;
    }
    let now_ms = now.get().saturating_mul(MILLIS_PER_SECOND);
    if now_ms < page.escalate_at_ms? {
        return None;
    }
    let step = page.step.saturating_add(1);
    let exhausted = usize::try_from(step).map_or(true, |step| step >= rotation.len());
    if exhausted {
        return Some(Escalation::Admins { step });
    }
    let target = match rotation.iter().position(|person| *person == page.target) {
        Some(at) => rotation.get((at + 1) % rotation.len()).copied(),
        None => rotation.first().copied(),
    }?;
    Some(Escalation::Next {
        target,
        step,
        escalate_at_ms: now_ms
            .saturating_add(escalate_after_secs.saturating_mul(MILLIS_PER_SECOND)),
    })
}

pub fn escalated(page: &Page, escalation: &Escalation) -> Page {
    let mut moved = page.clone();
    moved.state = PageState::Escalated;
    match escalation {
        Escalation::Next {
            target,
            step,
            escalate_at_ms,
        } => {
            moved.target = *target;
            moved.step = *step;
            moved.escalate_at_ms = Some(*escalate_at_ms);
        }
        Escalation::Admins { step } => {
            moved.step = *step;
            moved.escalate_at_ms = None;
        }
    }
    moved
}
