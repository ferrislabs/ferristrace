use super::entities::{RuleState, Transition};
use super::value_objects::{AlertStatus, Condition, NoDataPolicy};

const MILLIS_PER_SECOND: u64 = 1_000;

pub fn next_state(
    previous: Option<&RuleState>,
    observed: Option<f64>,
    condition: Condition,
    pending_secs: u64,
    no_data: NoDataPolicy,
    now_ms: u64,
) -> (RuleState, Option<Transition>) {
    let was = previous.map_or(AlertStatus::Ok, |held| held.status);
    let entered = previous.map_or(now_ms, |held| held.since_ms);
    let status = match observed {
        None => match no_data {
            NoDataPolicy::Ok => AlertStatus::Ok,
            NoDataPolicy::NoData => AlertStatus::NoData,
            NoDataPolicy::Firing => AlertStatus::Firing,
        },
        Some(value) if condition.holds(value) => match was {
            AlertStatus::Firing => AlertStatus::Firing,
            AlertStatus::Pending
                if now_ms.saturating_sub(entered) >= pending_secs * MILLIS_PER_SECOND =>
            {
                AlertStatus::Firing
            }
            AlertStatus::Ok | AlertStatus::NoData if pending_secs == 0 => AlertStatus::Firing,
            AlertStatus::Pending | AlertStatus::Ok | AlertStatus::NoData => AlertStatus::Pending,
        },
        Some(_) => AlertStatus::Ok,
    };
    let moved = status != was;
    let state = RuleState {
        status,
        value: observed,
        since_ms: if moved || previous.is_none() {
            now_ms
        } else {
            entered
        },
        evaluated_ms: now_ms,
    };
    let transition = moved.then_some(Transition {
        from: was,
        to: status,
        value: observed,
        at_ms: now_ms,
    });
    (state, transition)
}
