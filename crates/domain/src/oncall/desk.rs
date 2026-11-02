use super::ack::AckKey;
use super::entities::{Page, PageSource, PageView};
use super::errors::OncallError;
use super::ports::{Pages, Schedules};
use super::value_objects::{PageId, PageScope, PageState};
use crate::access::{ApiKeys, Reader, Readers};
use crate::agents::{AgentGrant, AgentScope};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, SessionStore};
use crate::incidents::{Author, Change, IncidentTimeline, Person, TimelineEntry, TimelineId};
use crate::organisation::{Memberships, OrgRole, Roll};
use crate::telemetry::TenantId;

const MILLIS_PER_SECOND: u64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Via {
    App,
    Link,
    Agent,
}

impl Via {
    fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Link => "link",
            Self::Agent => "agent",
        }
    }
}

enum Acker<'a> {
    Account(AccountId),
    Agent(&'a AgentGrant),
}

impl Acker<'_> {
    fn account(&self) -> Option<AccountId> {
        match self {
            Self::Account(account) => Some(*account),
            Self::Agent(_) => None,
        }
    }

    fn author(&self) -> Author {
        match self {
            Self::Account(account) => Author::Account(*account),
            Self::Agent(grant) => Author::Agent {
                id: Some(grant.agent),
                name: grant.name.clone(),
                on_behalf_of: grant
                    .delegated
                    .as_ref()
                    .map(|delegation| delegation.label().into()),
            },
        }
    }
}

#[derive(Debug)]
pub struct PageDesk<S, G, K, M, Sc, Pg, L, A> {
    readers: Readers<S, G, K>,
    roster: M,
    schedules: Sc,
    pages: Pg,
    timeline: L,
    trail: A,
}

impl<S, G, K, M, Sc, Pg, L, A> PageDesk<S, G, K, M, Sc, Pg, L, A>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    M: Roll,
    Sc: Schedules,
    Pg: Pages,
    L: IncidentTimeline,
    A: AuditTrail,
{
    pub fn new(
        readers: Readers<S, G, K>,
        roster: M,
        schedules: Sc,
        pages: Pg,
        timeline: L,
        trail: A,
    ) -> Self {
        Self {
            readers,
            roster,
            schedules,
            pages,
            timeline,
            trail,
        }
    }

    pub async fn list(
        &self,
        who: &Reader,
        org: TenantId,
        scope: PageScope,
    ) -> Result<Vec<PageView>, OncallError> {
        self.readers.grant(who, org).await?;
        let pages = self.pages.list(org, scope).await?;
        self.viewed(org, pages).await
    }

    pub async fn acknowledge(
        &self,
        who: &Reader,
        org: TenantId,
        id: PageId,
        now: UnixSeconds,
    ) -> Result<PageView, OncallError> {
        match who {
            Reader::Key(_) => {
                self.readers.grant(who, org).await?;
                Err(OncallError::ReadOnly)
            }
            Reader::Agent(call) => {
                let grant = self
                    .readers
                    .standing(call, org, Some(AgentScope::OncallAcknowledge))
                    .await?;
                if grant.project.is_some() {
                    return Err(OncallError::OutOfScope);
                }
                let page = self
                    .pages
                    .get(org, id)
                    .await?
                    .ok_or(OncallError::NoSuchPage)?;
                let page = self
                    .acknowledged(page, &Acker::Agent(&grant), Via::Agent, now)
                    .await?;
                self.viewed_one(org, page).await
            }
            Reader::Session(token) => {
                let me = self.readers.whoami().of(token).await?;
                let role = me.role_in(org).ok_or(OncallError::NotYours)?;
                let page = self
                    .pages
                    .get(org, id)
                    .await?
                    .ok_or(OncallError::NoSuchPage)?;
                if page.target != me.account && !role.at_least(OrgRole::Admin) {
                    return Err(OncallError::NotYourPage);
                }
                let page = self
                    .acknowledged(page, &Acker::Account(me.account), Via::App, now)
                    .await?;
                self.viewed_one(org, page).await
            }
        }
    }

    pub async fn acknowledge_link(
        &self,
        token: &str,
        key: Option<&AckKey>,
        now: UnixSeconds,
    ) -> Result<Page, OncallError> {
        let key = key.ok_or(OncallError::NoAckKey)?;
        let claim = key.open(token, now)?;
        let page = self
            .pages
            .get(claim.org, claim.page)
            .await?
            .ok_or(OncallError::NoSuchPage)?;
        self.acknowledged(page, &Acker::Account(claim.account), Via::Link, now)
            .await
    }

    async fn viewed_one(&self, org: TenantId, page: Page) -> Result<PageView, OncallError> {
        let mut viewed = self.viewed(org, vec![page]).await?;
        viewed.pop().ok_or(OncallError::NoSuchPage)
    }

    async fn acknowledged(
        &self,
        page: Page,
        by: &Acker<'_>,
        via: Via,
        now: UnixSeconds,
    ) -> Result<Page, OncallError> {
        if !page.state.awaits() {
            if let Acker::Agent(grant) = by {
                self.readers
                    .record(
                        grant,
                        &AuditEntry {
                            org: page.org,
                            actor: Actor::Agent(grant.agent),
                            action: Action::AgentRead,
                            target: Target::of(TargetKind::Page, Some(page.id.to_string().into())),
                            detail: grant.stamp(Detail::default()),
                        },
                    )
                    .await?;
            }
            return Ok(page);
        }
        let now_ms = now.get().saturating_mul(MILLIS_PER_SECOND);
        let mut moved = page;
        moved.state = PageState::Acknowledged;
        moved.acknowledged_by = by.account();
        moved.acknowledged_ms = Some(now_ms);
        moved.escalate_at_ms = None;
        if !self.pages.keep(&moved).await? {
            return Err(OncallError::NoSuchPage);
        }
        let target = Target::of(TargetKind::Page, Some(moved.id.to_string().into()));
        let detail = Detail::default()
            .with("via", via.as_str())
            .with("step", moved.step);
        match by {
            Acker::Account(account) => {
                self.trail
                    .record(&AuditEntry {
                        org: moved.org,
                        actor: Actor::Account(*account),
                        action: Action::PageAcknowledged,
                        target,
                        detail,
                    })
                    .await?;
            }
            Acker::Agent(grant) => {
                self.readers
                    .record(
                        grant,
                        &AuditEntry {
                            org: moved.org,
                            actor: Actor::Agent(grant.agent),
                            action: Action::PageAcknowledged,
                            target,
                            detail: grant.stamp(detail),
                        },
                    )
                    .await?;
            }
        }
        if let PageSource::Incident(incident) = moved.source {
            self.timeline
                .append(&TimelineEntry {
                    id: TimelineId::fresh(),
                    incident,
                    org: moved.org,
                    author: by.author(),
                    change: Change::Acknowledged { page: moved.id },
                    at_ms: now_ms,
                })
                .await?;
        }
        Ok(moved)
    }

    async fn viewed(&self, org: TenantId, pages: Vec<Page>) -> Result<Vec<PageView>, OncallError> {
        let schedules = self.schedules.of_org(org).await?;
        let members = self.roster.of(org).await?;
        Ok(pages
            .into_iter()
            .map(|page| {
                let target = members
                    .iter()
                    .find(|member| member.account == page.target)
                    .map_or(
                        Person {
                            account: page.target,
                            email: None,
                            name: None,
                        },
                        |member| Person {
                            account: member.account,
                            email: Some(member.email.clone()),
                            name: member.name.clone(),
                        },
                    );
                PageView {
                    schedule: schedules
                        .iter()
                        .find(|schedule| schedule.id == page.schedule)
                        .map(|schedule| schedule.name.clone()),
                    target,
                    page,
                }
            })
            .collect())
    }
}
