use std::collections::BTreeMap;

use super::courier::deliver;
use super::entities::{
    AlertEvent, Delivery, Dispatch, Preview, Rule, RuleDraft, RulePatch, RuleState, Watched,
};
use super::errors::{AlertError, BadSilence};
use super::ports::{
    AlertChannels, AlertEvents, AlertMailer, AlertRules, AlertStates, SignalProbe, WebhookSender,
};
use super::state_machine::next_state;
use super::value_objects::{
    AlertStatus, ChannelId, Definition, DefinitionParts, HISTORY_DAYS, Labels, RuleId,
};
use crate::access::{ApiKeys, ReadGrant, Reader, Readers};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::catalogue::{ProjectId, Projects};
use crate::clock::UnixSeconds;
use crate::explore::Window;
use crate::identity::{AccountId, SessionStore};
use crate::oncall::{ScheduleId, Schedules};
use crate::organisation::{Memberships, OrgRole};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

pub const PREVIEW_POINTS: u64 = 30;
pub const HISTORY_LIMIT: u32 = 1_000;
const MILLIS_PER_SECOND: u64 = 1_000;
const NANOS_PER_MILLI: u64 = 1_000_000;
const MILLIS_PER_DAY: u64 = 86_400_000;

#[derive(Debug)]
pub struct AlertBook<S, G, K, P, R, T, E, C, Sc, A> {
    readers: Readers<S, G, K>,
    projects: P,
    rules: R,
    states: T,
    events: E,
    channels: C,
    schedules: Sc,
    trail: A,
}

impl<S, G, K, P, R, T, E, C, Sc, A> AlertBook<S, G, K, P, R, T, E, C, Sc, A>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    P: Projects,
    R: AlertRules,
    T: AlertStates,
    E: AlertEvents,
    C: AlertChannels,
    Sc: Schedules,
    A: AuditTrail,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        readers: Readers<S, G, K>,
        projects: P,
        rules: R,
        states: T,
        events: E,
        channels: C,
        schedules: Sc,
        trail: A,
    ) -> Self {
        Self {
            readers,
            projects,
            rules,
            states,
            events,
            channels,
            schedules,
            trail,
        }
    }

    pub async fn list(&self, who: &Reader, org: TenantId) -> Result<Vec<Watched>, AlertError> {
        let grant = self.readers.grant(who, org).await?;
        let mut states = self.states.of_org(org).await?;
        Ok(self
            .rules
            .list(org)
            .await?
            .into_iter()
            .filter(|rule| visible(&grant, rule))
            .map(|rule| {
                let (mine, rest): (Vec<_>, Vec<_>) =
                    states.drain(..).partition(|(held, _)| *held == rule.id);
                states = rest;
                Watched {
                    series: mine.into_iter().map(|(_, state)| state).collect(),
                    rule,
                }
            })
            .collect())
    }

    pub async fn get(
        &self,
        who: &Reader,
        org: TenantId,
        id: RuleId,
    ) -> Result<Watched, AlertError> {
        let grant = self.readers.grant(who, org).await?;
        let rule = self.visible_rule(&grant, org, id).await?;
        self.watched(rule).await
    }

    pub async fn history(
        &self,
        who: &Reader,
        org: TenantId,
        id: RuleId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        now: UnixSeconds,
    ) -> Result<Vec<AlertEvent>, AlertError> {
        let grant = self.readers.grant(who, org).await?;
        let rule = self.visible_rule(&grant, org, id).await?;
        let to_ms = to_ms.unwrap_or(now.get() * MILLIS_PER_SECOND);
        let from_ms = from_ms.unwrap_or(to_ms.saturating_sub(HISTORY_DAYS * MILLIS_PER_DAY));
        Ok(self
            .events
            .history(org, rule.id, from_ms, to_ms, HISTORY_LIMIT)
            .await?)
    }

    pub async fn create(
        &self,
        token: &SessionToken,
        org: TenantId,
        draft: &RuleDraft,
    ) -> Result<Watched, AlertError> {
        let by = self.standing(token, org).await?;
        self.check_project(org, draft.project).await?;
        self.check_channels(org, draft.definition.channels())
            .await?;
        self.check_schedule(org, draft.definition.page()).await?;
        let rule = self
            .rules
            .create(org, draft, by)
            .await?
            .ok_or(AlertError::NameTaken)?;
        self.note(
            org,
            by,
            Action::AlertCreated,
            rule.id,
            Detail::default()
                .with("name", &rule.name)
                .with("signal", rule.definition.signal().kind())
                .with("enabled", rule.enabled),
        )
        .await?;
        Ok(Watched {
            rule,
            series: Vec::new(),
        })
    }

    pub async fn update(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: RuleId,
        patch: RulePatch,
    ) -> Result<Watched, AlertError> {
        let by = self.standing(token, org).await?;
        let held = self
            .rules
            .get(org, id)
            .await?
            .ok_or(AlertError::NoSuchRule)?;
        let changed = merged(&held, patch)?;
        if changed.project != held.project {
            self.check_project(org, changed.project).await?;
        }
        if changed.definition.channels() != held.definition.channels() {
            self.check_channels(org, changed.definition.channels())
                .await?;
        }
        if changed.definition.page() != held.definition.page() {
            self.check_schedule(org, changed.definition.page()).await?;
        }
        if changed.name != held.name
            && self
                .rules
                .list(org)
                .await?
                .iter()
                .any(|rule| rule.id != id && rule.name == changed.name)
        {
            return Err(AlertError::NameTaken);
        }
        if !self.rules.update(&changed).await? {
            return Err(AlertError::NoSuchRule);
        }
        let reshaped = changed.name != held.name
            || changed.project != held.project
            || changed.definition != held.definition;
        if reshaped {
            self.note(
                org,
                by,
                Action::AlertUpdated,
                id,
                Detail::default()
                    .with("name", &changed.name)
                    .with("signal", changed.definition.signal().kind()),
            )
            .await?;
        }
        if changed.enabled != held.enabled {
            let action = if changed.enabled {
                Action::AlertEnabled
            } else {
                Action::AlertDisabled
            };
            self.note(
                org,
                by,
                action,
                id,
                Detail::default().with("name", &changed.name),
            )
            .await?;
        }
        self.watched(changed).await
    }

    pub async fn delete(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: RuleId,
    ) -> Result<(), AlertError> {
        let by = self.standing(token, org).await?;
        let held = self
            .rules
            .get(org, id)
            .await?
            .ok_or(AlertError::NoSuchRule)?;
        if !self.rules.delete(org, id).await? {
            return Err(AlertError::NoSuchRule);
        }
        self.note(
            org,
            by,
            Action::AlertDeleted,
            id,
            Detail::default().with("name", &held.name),
        )
        .await
    }

    pub async fn silence(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: RuleId,
        until_ms: u64,
        now: UnixSeconds,
    ) -> Result<Watched, AlertError> {
        let by = self.standing(token, org).await?;
        let held = self
            .rules
            .get(org, id)
            .await?
            .ok_or(AlertError::NoSuchRule)?;
        validate_silence(until_ms, now.get() * MILLIS_PER_SECOND)?;
        let changed = Rule {
            silenced_until_ms: Some(until_ms),
            ..held
        };
        if !self.rules.update(&changed).await? {
            return Err(AlertError::NoSuchRule);
        }
        self.note(
            org,
            by,
            Action::AlertSilenced,
            id,
            Detail::default()
                .with("name", &changed.name)
                .with("until_ms", until_ms),
        )
        .await?;
        self.watched(changed).await
    }

    pub async fn unsilence(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: RuleId,
    ) -> Result<Watched, AlertError> {
        let by = self.standing(token, org).await?;
        let held = self
            .rules
            .get(org, id)
            .await?
            .ok_or(AlertError::NoSuchRule)?;
        if held.silenced_until_ms.is_none() {
            return self.watched(held).await;
        }
        let changed = Rule {
            silenced_until_ms: None,
            ..held
        };
        if !self.rules.update(&changed).await? {
            return Err(AlertError::NoSuchRule);
        }
        self.note(
            org,
            by,
            Action::AlertUnsilenced,
            id,
            Detail::default().with("name", &changed.name),
        )
        .await?;
        self.watched(changed).await
    }

    pub async fn preview<Q: SignalProbe>(
        &self,
        who: &Reader,
        org: TenantId,
        project: Option<ProjectId>,
        definition: &Definition,
        probe: &Q,
        now: UnixSeconds,
    ) -> Result<Preview, AlertError> {
        let project = self.readers.grant(who, org).await?.narrow(project)?;
        let window_ms = definition.window_secs() * MILLIS_PER_SECOND;
        let step_ms = window_ms.div_ceil(PREVIEW_POINTS).max(MILLIS_PER_SECOND);
        let now_ms = now.get() * MILLIS_PER_SECOND;
        let condition = definition.condition();
        let mut held: BTreeMap<Labels, RuleState> = BTreeMap::new();
        let mut points = Vec::new();
        let mut would_fire = false;
        for slot in (0..PREVIEW_POINTS).rev() {
            let at_ms = now_ms.saturating_sub(slot * step_ms);
            let window = window_ending(at_ms, definition.window_secs())?;
            let observed = probe
                .observe(org, project, definition.signal(), window)
                .await?;
            let mut worst: Option<f64> = None;
            let mut firing = false;
            for observation in seen_or_silent(observed) {
                if let Some(value) = observation.value {
                    worst = Some(worst.map_or(value, |kept| condition.op().worst(kept, value)));
                }
                let (state, _) = next_state(
                    held.get(&observation.labels),
                    observation.value,
                    condition,
                    definition.pending_secs(),
                    definition.no_data(),
                    at_ms,
                );
                firing |= state.status == AlertStatus::Firing;
                held.insert(observation.labels, state);
            }
            would_fire = firing;
            points.push((at_ms * NANOS_PER_MILLI, worst));
        }
        Ok(Preview {
            step_nanos: step_ms * NANOS_PER_MILLI,
            points,
            threshold: condition.threshold(),
            would_fire,
        })
    }

    pub async fn rehearse<M: AlertMailer, W: WebhookSender>(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: RuleId,
        mailer: &M,
        webhooks: &W,
        now: UnixSeconds,
    ) -> Result<Vec<Delivery>, AlertError> {
        self.standing(token, org).await?;
        let rule = self
            .rules
            .get(org, id)
            .await?
            .ok_or(AlertError::NoSuchRule)?;
        let dispatch = Dispatch {
            rule: rule.id,
            name: rule.name.as_str().into(),
            severity: rule.definition.severity(),
            org,
            from: AlertStatus::Ok,
            to: AlertStatus::Firing,
            value: None,
            labels: Labels::new(BTreeMap::from([("test".into(), "true".into())])),
            at_ms: now.get() * MILLIS_PER_SECOND,
            context: None,
            mentions: rule.definition.mentions().clone(),
        };
        let mut deliveries = Vec::new();
        for channel in rule.definition.channels() {
            let Some(armed) = self.channels.armed(org, *channel).await? else {
                continue;
            };
            deliveries.push(Delivery {
                channel: *channel,
                outcome: deliver(mailer, webhooks, &armed, &dispatch).await,
            });
        }
        Ok(deliveries)
    }

    async fn watched(&self, rule: Rule) -> Result<Watched, AlertError> {
        let series = self.states.of(rule.id).await?;
        Ok(Watched { rule, series })
    }

    async fn visible_rule(
        &self,
        grant: &ReadGrant,
        org: TenantId,
        id: RuleId,
    ) -> Result<Rule, AlertError> {
        self.rules
            .get(org, id)
            .await?
            .filter(|rule| visible(grant, rule))
            .ok_or(AlertError::NoSuchRule)
    }

    async fn check_project(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
    ) -> Result<(), AlertError> {
        let Some(project) = project else {
            return Ok(());
        };
        if self
            .projects
            .of(org)
            .await?
            .iter()
            .any(|held| held.id == project)
        {
            Ok(())
        } else {
            Err(AlertError::NoSuchProject)
        }
    }

    async fn check_channels(&self, org: TenantId, asked: &[ChannelId]) -> Result<(), AlertError> {
        if asked.is_empty() {
            return Ok(());
        }
        let held = self.channels.list(org).await?;
        if asked
            .iter()
            .all(|channel| held.iter().any(|kept| kept.id == *channel))
        {
            Ok(())
        } else {
            Err(AlertError::UnknownChannel)
        }
    }

    async fn check_schedule(
        &self,
        org: TenantId,
        page: Option<ScheduleId>,
    ) -> Result<(), AlertError> {
        let Some(schedule) = page else {
            return Ok(());
        };
        self.schedules
            .get(org, schedule)
            .await?
            .map(|_| ())
            .ok_or(AlertError::UnknownSchedule)
    }

    async fn standing(&self, token: &SessionToken, org: TenantId) -> Result<AccountId, AlertError> {
        standing(self.readers.whoami(), token, org).await
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        rule: RuleId,
        detail: Detail,
    ) -> Result<(), AlertError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(by),
                action,
                target: Target::of(TargetKind::Alert, Some(rule.to_string().into())),
                detail,
            })
            .await?)
    }
}

pub(super) async fn standing<S: SessionStore, G: Memberships>(
    whoami: &crate::organisation::Whoami<S, G>,
    token: &SessionToken,
    org: TenantId,
) -> Result<AccountId, AlertError> {
    let who = whoami.of(token).await?;
    match who.role_in(org) {
        None => Err(AlertError::NotYours),
        Some(role) if !role.at_least(OrgRole::Admin) => Err(AlertError::NotYoursToChange),
        Some(_) => Ok(who.account),
    }
}

fn visible(grant: &ReadGrant, rule: &Rule) -> bool {
    grant.project.is_none() || rule.project == grant.project
}

fn merged(held: &Rule, patch: RulePatch) -> Result<Rule, AlertError> {
    let base = held.definition.parts();
    let parts = DefinitionParts {
        signal: patch.signal.unwrap_or_else(|| base.signal.clone()),
        condition: patch.condition.unwrap_or(base.condition),
        window_secs: patch.window_secs.unwrap_or(base.window_secs),
        pending_secs: patch.pending_secs.unwrap_or(base.pending_secs),
        severity: patch.severity.unwrap_or(base.severity),
        no_data: patch.no_data.unwrap_or(base.no_data),
        channels: patch.channels.unwrap_or_else(|| base.channels.clone()),
        page: patch.page.unwrap_or(base.page),
        mentions: patch.mentions.unwrap_or_else(|| base.mentions.clone()),
    };
    Ok(Rule {
        id: held.id,
        org: held.org,
        name: patch.name.unwrap_or_else(|| held.name.clone()),
        project: patch.project.unwrap_or(held.project),
        enabled: patch.enabled.unwrap_or(held.enabled),
        definition: Definition::new(parts)?,
        created_ms: held.created_ms,
        silenced_until_ms: held.silenced_until_ms,
    })
}

const SILENCE_MAX_DAYS: u32 = 30;

fn validate_silence(until_ms: u64, now_ms: u64) -> Result<(), BadSilence> {
    if until_ms <= now_ms {
        return Err(BadSilence::InThePast);
    }
    if until_ms - now_ms > u64::from(SILENCE_MAX_DAYS) * MILLIS_PER_DAY {
        return Err(BadSilence::TooLong {
            max_days: SILENCE_MAX_DAYS,
        });
    }
    Ok(())
}

pub(super) fn window_ending(at_ms: u64, window_secs: u64) -> Result<Window, AlertError> {
    let to = at_ms.saturating_mul(NANOS_PER_MILLI);
    let from = to.saturating_sub(window_secs * MILLIS_PER_SECOND * NANOS_PER_MILLI);
    Window::new(from, to).map_err(|_| AlertError::StoreUnreachable)
}

pub(super) fn seen_or_silent(
    observed: Vec<super::entities::Observation>,
) -> Vec<super::entities::Observation> {
    if observed.is_empty() {
        vec![super::entities::Observation {
            labels: Labels::none(),
            value: None,
        }]
    } else {
        observed
    }
}
