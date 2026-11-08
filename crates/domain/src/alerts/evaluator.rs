use super::alert_book::{seen_or_silent, window_ending};
use super::culprits::Culprits;
use super::entities::{AlertEvent, Notification, Observation, Rule, SeriesState, Tick, Transition};
use super::errors::AlertError;
use super::ports::{AlertDuty, AlertEvents, AlertOutbox, AlertRules, AlertStates, SignalProbe};
use super::state_machine::next_state;
use super::value_objects::{
    AlertStatus, DeliveryStatus, Duty, EventId, HISTORY_DAYS, Labels, NotificationId,
};
use crate::clock::UnixSeconds;
use crate::explore::Window;
use crate::incidents::{Followers, IncidentTimeline, Incidents};
use crate::oncall::{Origin, PageSource, Pages, Paging, Schedules};

pub const TICK_SECS: u64 = 60;
const MILLIS_PER_SECOND: u64 = 1_000;
const MILLIS_PER_DAY: u64 = 86_400_000;

#[derive(Debug)]
pub struct Evaluator<R, T, E, O, D, P, I, L, Sc, Pg> {
    rules: R,
    states: T,
    events: E,
    outbox: O,
    duty: D,
    probe: P,
    followers: Followers<I, L>,
    paging: Paging<Sc, Pg>,
}

impl<R, T, E, O, D, P, I, L, Sc, Pg> Evaluator<R, T, E, O, D, P, I, L, Sc, Pg>
where
    R: AlertRules,
    T: AlertStates,
    E: AlertEvents,
    O: AlertOutbox,
    D: AlertDuty,
    P: SignalProbe,
    I: Incidents,
    L: IncidentTimeline,
    Sc: Schedules,
    Pg: Pages,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rules: R,
        states: T,
        events: E,
        outbox: O,
        duty: D,
        probe: P,
        followers: Followers<I, L>,
        paging: Paging<Sc, Pg>,
    ) -> Self {
        Self {
            rules,
            states,
            events,
            outbox,
            duty,
            probe,
            followers,
            paging,
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

    async fn culprits_of(
        &self,
        rule: &Rule,
        transition: &Transition,
        window: Window,
        labels: &Labels,
    ) -> Option<Culprits> {
        if transition.to != AlertStatus::Firing || !transition.notifies() {
            return None;
        }
        self.probe
            .culprits(
                rule.org,
                rule.project,
                rule.definition.signal(),
                window,
                labels,
            )
            .await
            .ok()
            .filter(|culprits| !culprits.is_empty())
    }

    async fn dispatch(
        &self,
        rule: &Rule,
        transition: &Transition,
        event: &AlertEvent,
        silenced: bool,
        now_ms: u64,
    ) -> Result<(), AlertError> {
        let definition = &rule.definition;
        if !silenced
            && let (AlertStatus::Firing, Some(schedule)) = (transition.to, definition.page())
        {
            self.paging
                .raise(
                    rule.org,
                    schedule,
                    PageSource::Alert {
                        rule: rule.id,
                        event: event.id,
                    },
                    rule.name.as_str(),
                    now_ms,
                )
                .await?;
        }
        if !transition.notifies() || silenced {
            return Ok(());
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
        Ok(())
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
        let silenced = rule.silenced(now_ms);
        let mut moved = 0;
        let (mut firing, mut left_firing) = (false, false);
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
            firing |= state.status == AlertStatus::Firing;
            let Some(transition) = transition else {
                continue;
            };
            left_firing |= transition.from == AlertStatus::Firing;
            moved += 1;
            let context = self
                .culprits_of(rule, &transition, window, &observation.labels)
                .await;
            let event = AlertEvent {
                id: EventId::fresh(),
                rule: rule.id,
                org: rule.org,
                labels: observation.labels,
                from: transition.from,
                to: transition.to,
                value: transition.value,
                at_ms: transition.at_ms,
                context,
            };
            self.events.record(&event).await?;
            self.followers.tell(&event).await?;
            self.dispatch(rule, &transition, &event, silenced, now_ms)
                .await?;
        }
        if left_firing && !firing {
            self.paging
                .resolve(rule.org, Origin::Rule(rule.id), now_ms)
                .await?;
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
