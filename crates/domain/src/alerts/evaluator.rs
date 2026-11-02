use super::alert_book::{seen_or_silent, window_ending};
use super::entities::{AlertEvent, Notification, Observation, Rule, SeriesState, Tick};
use super::errors::AlertError;
use super::ports::{AlertDuty, AlertEvents, AlertOutbox, AlertRules, AlertStates, SignalProbe};
use super::state_machine::next_state;
use super::value_objects::{DeliveryStatus, Duty, EventId, HISTORY_DAYS, NotificationId};
use crate::clock::UnixSeconds;
use crate::incidents::{Followers, IncidentTimeline, Incidents};

pub const TICK_SECS: u64 = 60;
const MILLIS_PER_SECOND: u64 = 1_000;
const MILLIS_PER_DAY: u64 = 86_400_000;

#[derive(Debug)]
pub struct Evaluator<R, T, E, O, D, P, I, L> {
    rules: R,
    states: T,
    events: E,
    outbox: O,
    duty: D,
    probe: P,
    followers: Followers<I, L>,
}

impl<R, T, E, O, D, P, I, L> Evaluator<R, T, E, O, D, P, I, L>
where
    R: AlertRules,
    T: AlertStates,
    E: AlertEvents,
    O: AlertOutbox,
    D: AlertDuty,
    P: SignalProbe,
    I: Incidents,
    L: IncidentTimeline,
{
    pub fn new(
        rules: R,
        states: T,
        events: E,
        outbox: O,
        duty: D,
        probe: P,
        followers: Followers<I, L>,
    ) -> Self {
        Self {
            rules,
            states,
            events,
            outbox,
            duty,
            probe,
            followers,
        }
    }

    pub async fn tick(&self, now: UnixSeconds) -> Result<Tick, AlertError> {
        if !self
            .duty
            .lead(Duty::Evaluate, now.get() / TICK_SECS)
            .await?
        {
            return Ok(Tick::Skipped);
        }
        let now_ms = now.get() * MILLIS_PER_SECOND;
        let pruned = self
            .events
            .prune(now_ms.saturating_sub(HISTORY_DAYS * MILLIS_PER_DAY))
            .await?;
        let rules = self.rules.enabled().await?;
        let (mut transitions, mut failed) = (0, 0);
        for rule in &rules {
            match self.evaluate(rule, now_ms).await? {
                Some(moved) => transitions += moved,
                None => failed += 1,
            }
        }
        Ok(Tick::Evaluated {
            rules: rules.len(),
            transitions,
            failed,
            pruned,
        })
    }

    async fn evaluate(&self, rule: &Rule, now_ms: u64) -> Result<Option<usize>, AlertError> {
        let definition = &rule.definition;
        let window = window_ending(now_ms, definition.window_secs())?;
        let Ok(observed) = self
            .probe
            .observe(rule.org, rule.project, definition.signal(), window)
            .await
        else {
            return Ok(None);
        };
        let previous = self.states.of(rule.id).await?;
        let mut moved = 0;
        for observation in alongside(observed, &previous) {
            let held = previous
                .iter()
                .find(|series| series.labels == observation.labels)
                .map(|series| &series.state);
            let (state, transition) = next_state(
                held,
                observation.value,
                definition.condition(),
                definition.pending_secs(),
                definition.no_data(),
                now_ms,
            );
            self.states
                .keep(
                    rule.id,
                    &SeriesState {
                        labels: observation.labels.clone(),
                        state,
                    },
                )
                .await?;
            let Some(transition) = transition else {
                continue;
            };
            moved += 1;
            let event = AlertEvent {
                id: EventId::fresh(),
                rule: rule.id,
                org: rule.org,
                labels: observation.labels,
                from: transition.from,
                to: transition.to,
                value: transition.value,
                at_ms: transition.at_ms,
            };
            self.events.record(&event).await?;
            self.followers.tell(&event).await?;
            if !transition.notifies() {
                continue;
            }
            for channel in definition.channels() {
                self.outbox
                    .enqueue(&Notification {
                        id: NotificationId::fresh(),
                        org: rule.org,
                        rule: rule.id,
                        event: event.id,
                        channel: *channel,
                        attempt: 0,
                        next_at_ms: now_ms,
                        status: DeliveryStatus::Pending,
                        last_error: None,
                    })
                    .await?;
            }
        }
        Ok(Some(moved))
    }
}

fn alongside(observed: Vec<Observation>, previous: &[SeriesState]) -> Vec<Observation> {
    let fresh = !observed.is_empty();
    let mut all = seen_or_silent(observed);
    for series in previous {
        if fresh && series.labels.is_empty() {
            continue;
        }
        if !all.iter().any(|seen| seen.labels == series.labels) {
            all.push(Observation {
                labels: series.labels.clone(),
                value: None,
            });
        }
    }
    all
}
