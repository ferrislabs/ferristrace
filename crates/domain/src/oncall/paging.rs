use super::ack::{AckClaim, AckKey};
use super::entities::{Escalation, Notice, Origin, Page, PageSource, Paged};
use super::errors::OncallError;
use super::escalation::{escalated, next};
use super::ports::{Pages, Schedules};
use super::rotation::on_call;
use super::value_objects::{
    ACK_LIFETIME_SECS, ESCALATE_AFTER_SECS, NoticeId, PAGE_TICK_SECS, PageId, PageState, ScheduleId,
};
use crate::alerts::{
    AlertDuty, AlertMailer, DRAIN_BATCH, DeliveryStatus, Duty, Settlement, settlement_after,
};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::incidents::{Author, Change, IncidentTimeline, TimelineEntry, TimelineId};
use crate::organisation::{OrgRole, Roll};
use crate::store::StoreError;
use crate::telemetry::TenantId;

const MILLIS_PER_SECOND: u64 = 1_000;

#[derive(Debug)]
pub struct Paging<Sc, Pg> {
    schedules: Sc,
    pages: Pg,
    escalate_after_secs: u64,
}

impl<Sc, Pg> Paging<Sc, Pg>
where
    Sc: Schedules,
    Pg: Pages,
{
    pub fn new(schedules: Sc, pages: Pg) -> Self {
        Self {
            schedules,
            pages,
            escalate_after_secs: ESCALATE_AFTER_SECS,
        }
    }

    #[must_use]
    pub fn escalating_after(mut self, secs: u64) -> Self {
        self.escalate_after_secs = secs;
        self
    }

    pub fn schedules(&self) -> &Sc {
        &self.schedules
    }

    pub fn pages(&self) -> &Pg {
        &self.pages
    }

    pub async fn raise(
        &self,
        org: TenantId,
        schedule: ScheduleId,
        source: PageSource,
        headline: &str,
        at_ms: u64,
    ) -> Result<Option<Page>, StoreError> {
        let Some(held) = self.schedules.get(org, schedule).await? else {
            return Ok(None);
        };
        let overrides = self.schedules.overrides(org, schedule).await?;
        let Some(target) = on_call(
            &held,
            &overrides,
            UnixSeconds::new(at_ms / MILLIS_PER_SECOND),
        ) else {
            return Ok(None);
        };
        let page = Page {
            id: PageId::fresh(),
            org,
            schedule,
            target,
            source,
            headline: headline.into(),
            state: PageState::Triggered,
            step: 0,
            created_ms: at_ms,
            escalate_at_ms: Some(
                at_ms.saturating_add(self.escalate_after_secs.saturating_mul(MILLIS_PER_SECOND)),
            ),
            acknowledged_by: None,
            acknowledged_ms: None,
            resolved_ms: None,
        };
        if !self.pages.raise(&page).await? {
            return Ok(None);
        }
        self.notify(&page, target, at_ms).await?;
        Ok(Some(page))
    }

    pub async fn resolve(
        &self,
        org: TenantId,
        origin: Origin,
        at_ms: u64,
    ) -> Result<Vec<Page>, StoreError> {
        self.pages.resolve(org, origin, at_ms).await
    }

    async fn notify(&self, page: &Page, to: AccountId, at_ms: u64) -> Result<(), StoreError> {
        self.pages
            .enqueue(&Notice {
                id: NoticeId::fresh(),
                org: page.org,
                page: page.id,
                step: page.step,
                to,
                attempt: 0,
                next_at_ms: at_ms,
                status: DeliveryStatus::Pending,
                last_error: None,
            })
            .await
    }
}

#[derive(Debug)]
pub struct Pager<'a, Sc, Pg, D, R, L, A, M> {
    paging: Paging<Sc, Pg>,
    duty: D,
    roster: R,
    timeline: L,
    trail: A,
    mailer: &'a M,
    key: Option<&'a AckKey>,
}

impl<'a, Sc, Pg, D, R, L, A, M> Pager<'a, Sc, Pg, D, R, L, A, M>
where
    Sc: Schedules,
    Pg: Pages,
    D: AlertDuty,
    R: Roll,
    L: IncidentTimeline,
    A: AuditTrail,
    M: AlertMailer + Sync,
{
    pub fn new(
        paging: Paging<Sc, Pg>,
        duty: D,
        roster: R,
        timeline: L,
        trail: A,
        mailer: &'a M,
        key: Option<&'a AckKey>,
    ) -> Self {
        Self {
            paging,
            duty,
            roster,
            timeline,
            trail,
            mailer,
            key,
        }
    }

    pub async fn tick(&self, now: UnixSeconds) -> Result<Paged, OncallError> {
        if !self
            .duty
            .lead(Duty::Escalate, now.get() / PAGE_TICK_SECS)
            .await?
        {
            return Ok(Paged::Skipped);
        }
        let now_ms = now.get().saturating_mul(MILLIS_PER_SECOND);
        let mut escalated = 0;
        for page in self.paging.pages.due(now_ms, DRAIN_BATCH).await? {
            if self.escalate(&page, now).await? {
                escalated += 1;
            }
        }
        let (mut sent, mut retried, mut abandoned) = (0, 0, 0);
        for claimed in self.paging.pages.claim(now_ms, DRAIN_BATCH).await? {
            let attempts = claimed.notice.attempt + 1;
            let mut dispatch = claimed.dispatch;
            dispatch.ack = self.key.map(|key| {
                key.sign(
                    &AckClaim {
                        org: claimed.notice.org,
                        page: claimed.notice.page,
                        account: claimed.notice.to,
                    },
                    now.saturating_add_secs(ACK_LIFETIME_SECS),
                )
            });
            let settlement = match self.mailer.send_page(&claimed.email, &dispatch).await {
                Ok(()) => Settlement::Sent,
                Err(error) => settlement_after(attempts, now_ms, &error),
            };
            match settlement {
                Settlement::Sent => sent += 1,
                Settlement::Retry { .. } => retried += 1,
                Settlement::Abandoned { .. } => abandoned += 1,
            }
            self.paging
                .pages
                .settle(claimed.notice.id, attempts, &settlement)
                .await?;
        }
        Ok(Paged::Worked {
            escalated,
            sent,
            retried,
            abandoned,
        })
    }

    async fn escalate(&self, page: &Page, now: UnixSeconds) -> Result<bool, OncallError> {
        let rotation = self
            .paging
            .schedules
            .get(page.org, page.schedule)
            .await?
            .map(|schedule| schedule.rotation.people().to_vec())
            .unwrap_or_default();
        let Some(escalation) = next(page, &rotation, now, self.paging.escalate_after_secs) else {
            return Ok(false);
        };
        let moved = escalated(page, &escalation);
        if !self.paging.pages.keep(&moved).await? {
            return Ok(false);
        }
        let now_ms = now.get().saturating_mul(MILLIS_PER_SECOND);
        let (target, recipients) = match escalation {
            Escalation::Next { target, .. } => (Some(target), vec![target]),
            Escalation::Admins { .. } => (
                None,
                self.roster
                    .of(page.org)
                    .await?
                    .into_iter()
                    .filter(|member| member.role.at_least(OrgRole::Admin))
                    .map(|member| member.account)
                    .collect(),
            ),
        };
        for to in &recipients {
            self.paging.notify(&moved, *to, now_ms).await?;
        }
        self.trail
            .record(&AuditEntry {
                org: page.org,
                actor: Actor::System,
                action: Action::PageEscalated,
                target: Target::of(TargetKind::Page, Some(page.id.to_string().into())),
                detail: Detail::default()
                    .with("step", moved.step)
                    .with_all("to", &recipients),
            })
            .await?;
        if let PageSource::Incident(incident) = page.source {
            self.timeline
                .append(&TimelineEntry {
                    id: TimelineId::fresh(),
                    incident,
                    org: page.org,
                    author: Author::System,
                    change: Change::Paged {
                        page: page.id,
                        step: moved.step,
                        target,
                    },
                    at_ms: now_ms,
                })
                .await?;
        }
        Ok(true)
    }
}
