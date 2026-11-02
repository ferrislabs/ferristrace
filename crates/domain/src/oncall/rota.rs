use super::entities::{OnCallNow, Override, Rostered, Schedule, ScheduleDraft, SchedulePatch};
use super::errors::{BadSchedule, OncallError};
use super::ports::Schedules;
use super::rotation::shift;
use super::value_objects::{OverrideId, ScheduleId};
use crate::access::{ApiKeys, Reader, Readers};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, SessionStore};
use crate::incidents::Person;
use crate::organisation::{Memberships, OrgRole, Roll, Squads, Team, TeamId};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

const MILLIS_PER_SECOND: u64 = 1_000;

#[derive(Debug)]
pub struct Rota<S, G, K, Q, M, Sc, A> {
    readers: Readers<S, G, K>,
    squads: Q,
    roster: M,
    schedules: Sc,
    trail: A,
}

impl<S, G, K, Q, M, Sc, A> Rota<S, G, K, Q, M, Sc, A>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    Q: Squads,
    M: Roll,
    Sc: Schedules,
    A: AuditTrail,
{
    pub fn new(readers: Readers<S, G, K>, squads: Q, roster: M, schedules: Sc, trail: A) -> Self {
        Self {
            readers,
            squads,
            roster,
            schedules,
            trail,
        }
    }

    pub async fn list(
        &self,
        who: &Reader,
        org: TenantId,
        team: TeamId,
        now: UnixSeconds,
    ) -> Result<Vec<Rostered>, OncallError> {
        self.readers.grant(who, org).await?;
        self.team(org, team).await?;
        let mut rostered = Vec::new();
        for schedule in self.schedules.of_team(org, team).await? {
            rostered.push(self.rostered(schedule, now).await?);
        }
        Ok(rostered)
    }

    pub async fn get(
        &self,
        who: &Reader,
        org: TenantId,
        id: ScheduleId,
        now: UnixSeconds,
    ) -> Result<Rostered, OncallError> {
        self.readers.grant(who, org).await?;
        let schedule = self.held(org, id).await?;
        self.rostered(schedule, now).await
    }

    pub async fn now(
        &self,
        who: &Reader,
        org: TenantId,
        id: ScheduleId,
        now: UnixSeconds,
    ) -> Result<OnCallNow, OncallError> {
        self.readers.grant(who, org).await?;
        let schedule = self.held(org, id).await?;
        let overrides = self.schedules.overrides(org, id).await?;
        let Some(held) = shift(&schedule, &overrides, now) else {
            return Ok(OnCallNow {
                person: None,
                until_ms: None,
            });
        };
        let person = self
            .roster
            .of(org)
            .await?
            .into_iter()
            .find(|member| member.account == held.account)
            .map_or(
                Person {
                    account: held.account,
                    email: None,
                    name: None,
                },
                |member| Person {
                    account: member.account,
                    email: Some(member.email),
                    name: member.name,
                },
            );
        Ok(OnCallNow {
            person: Some(person),
            until_ms: held.until_ms,
        })
    }

    pub async fn create(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        draft: &ScheduleDraft,
        now: UnixSeconds,
    ) -> Result<Rostered, OncallError> {
        let by = self.standing(token, org).await?;
        let held = self.team(org, team).await?;
        seated(&held, draft.rotation.people())?;
        let schedule = self
            .schedules
            .create(org, team, draft)
            .await?
            .ok_or(OncallError::NameTaken)?;
        self.note(
            org,
            by,
            Action::ScheduleCreated,
            schedule.id,
            Detail::default()
                .with("name", &schedule.name)
                .with("team", team)
                .with("timezone", &schedule.timezone)
                .with_all("rotation", schedule.rotation.people()),
        )
        .await?;
        self.rostered(schedule, now).await
    }

    pub async fn update(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ScheduleId,
        patch: SchedulePatch,
        now: UnixSeconds,
    ) -> Result<Rostered, OncallError> {
        let by = self.standing(token, org).await?;
        let held = self.held(org, id).await?;
        let changed = merged(&held, patch);
        let mut fields = Vec::new();
        if changed.name != held.name {
            fields.push("name");
            if self
                .schedules
                .of_team(org, held.team)
                .await?
                .iter()
                .any(|other| other.id != id && other.name == changed.name)
            {
                return Err(OncallError::NameTaken);
            }
        }
        if changed.rotation != held.rotation {
            fields.push("rotation");
            let team = self.team(org, held.team).await?;
            seated(&team, changed.rotation.people())?;
        }
        if changed.timezone != held.timezone {
            fields.push("timezone");
        }
        if changed.handoff != held.handoff {
            fields.push("handoff");
        }
        if changed.length != held.length {
            fields.push("length_days");
        }
        if changed.starts_on != held.starts_on {
            fields.push("starts_on");
        }
        if fields.is_empty() {
            return self.rostered(held, now).await;
        }
        if !self.schedules.update(&changed).await? {
            return Err(OncallError::NoSuchSchedule);
        }
        self.note(
            org,
            by,
            Action::ScheduleUpdated,
            id,
            Detail::default()
                .with("name", &changed.name)
                .with_all("fields", &fields),
        )
        .await?;
        self.rostered(changed, now).await
    }

    pub async fn delete(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ScheduleId,
    ) -> Result<(), OncallError> {
        let by = self.standing(token, org).await?;
        let held = self.held(org, id).await?;
        if !self.schedules.delete(org, id).await? {
            return Err(OncallError::NoSuchSchedule);
        }
        self.note(
            org,
            by,
            Action::ScheduleDeleted,
            id,
            Detail::default().with("name", &held.name),
        )
        .await
    }

    pub async fn add_override(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ScheduleId,
        account: AccountId,
        from_ms: u64,
        to_ms: u64,
    ) -> Result<Override, OncallError> {
        let by = self.standing(token, org).await?;
        self.held(org, id).await?;
        if from_ms >= to_ms {
            return Err(BadSchedule::Window.into());
        }
        if !self
            .roster
            .of(org)
            .await?
            .iter()
            .any(|member| member.account == account)
        {
            return Err(OncallError::NotAMember);
        }
        let cover = Override {
            id: OverrideId::fresh(),
            schedule: id,
            account,
            from_ms,
            to_ms,
        };
        self.schedules.add_override(org, &cover).await?;
        self.note(
            org,
            by,
            Action::OverrideCreated,
            id,
            Detail::default()
                .with("override", cover.id)
                .with("account", account)
                .with("from_ms", from_ms)
                .with("to_ms", to_ms),
        )
        .await?;
        Ok(cover)
    }

    pub async fn remove_override(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ScheduleId,
        cover: OverrideId,
    ) -> Result<(), OncallError> {
        let by = self.standing(token, org).await?;
        self.held(org, id).await?;
        if !self.schedules.remove_override(org, id, cover).await? {
            return Err(OncallError::NoSuchOverride);
        }
        self.note(
            org,
            by,
            Action::OverrideDeleted,
            id,
            Detail::default().with("override", cover),
        )
        .await
    }

    async fn rostered(
        &self,
        schedule: Schedule,
        now: UnixSeconds,
    ) -> Result<Rostered, OncallError> {
        let now_ms = now.get().saturating_mul(MILLIS_PER_SECOND);
        let overrides = self
            .schedules
            .overrides(schedule.org, schedule.id)
            .await?
            .into_iter()
            .filter(|cover| cover.to_ms > now_ms)
            .collect();
        Ok(Rostered {
            schedule,
            overrides,
        })
    }

    async fn held(&self, org: TenantId, id: ScheduleId) -> Result<Schedule, OncallError> {
        self.schedules
            .get(org, id)
            .await?
            .ok_or(OncallError::NoSuchSchedule)
    }

    async fn team(&self, org: TenantId, team: TeamId) -> Result<Team, OncallError> {
        self.squads
            .of(org)
            .await?
            .into_iter()
            .find(|held| held.id == team)
            .ok_or(OncallError::NoSuchTeam)
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<AccountId, OncallError> {
        let who = self.readers.whoami().of(token).await?;
        match who.role_in(org) {
            None => Err(OncallError::NotYours),
            Some(role) if !role.at_least(OrgRole::Admin) => Err(OncallError::NotYoursToChange),
            Some(_) => Ok(who.account),
        }
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        schedule: ScheduleId,
        detail: Detail,
    ) -> Result<(), OncallError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(by),
                action,
                target: Target::of(TargetKind::Schedule, Some(schedule.to_string().into())),
                detail,
            })
            .await?)
    }
}

fn seated(team: &Team, rotation: &[AccountId]) -> Result<(), OncallError> {
    if rotation.iter().all(|person| team.members.contains(person)) {
        Ok(())
    } else {
        Err(OncallError::NotInTeam)
    }
}

fn merged(held: &Schedule, patch: SchedulePatch) -> Schedule {
    Schedule {
        id: held.id,
        org: held.org,
        team: held.team,
        name: patch.name.unwrap_or_else(|| held.name.clone()),
        timezone: patch.timezone.unwrap_or_else(|| held.timezone.clone()),
        rotation: patch.rotation.unwrap_or_else(|| held.rotation.clone()),
        handoff: patch.handoff.unwrap_or(held.handoff),
        length: patch.length.unwrap_or(held.length),
        starts_on: patch.starts_on.unwrap_or(held.starts_on),
    }
}
