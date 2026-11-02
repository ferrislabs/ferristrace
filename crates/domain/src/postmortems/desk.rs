use super::drafting::draft;
use super::entities::{Postmortem, PostmortemSummary, Written};
use super::errors::PostmortemError;
use super::ports::Postmortems;
use super::value_objects::{PostmortemBody, PostmortemId, PostmortemStatus};
use crate::access::{ApiKeys, ReadGrant, Reader, Readers};
use crate::alerts::AlertRules;
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, SessionStore};
use crate::incidents::{
    AlertRef, Chronicle, Incident, IncidentAttachments, IncidentId, IncidentTimeline, Incidents,
    Person,
};
use crate::organisation::{Memberships, OrgRole, Roll};
use crate::telemetry::TenantId;

const MILLIS_PER_SECOND: u64 = 1_000;

#[derive(Debug)]
pub struct PostmortemDesk<S, G, K, I, L, N, M, R, P, A> {
    readers: Readers<S, G, K>,
    incidents: I,
    timeline: L,
    attachments: N,
    roster: M,
    rules: R,
    postmortems: P,
    trail: A,
}

impl<S, G, K, I, L, N, M, R, P, A> PostmortemDesk<S, G, K, I, L, N, M, R, P, A>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    I: Incidents,
    L: IncidentTimeline,
    N: IncidentAttachments,
    M: Roll,
    R: AlertRules,
    P: Postmortems,
    A: AuditTrail,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        readers: Readers<S, G, K>,
        incidents: I,
        timeline: L,
        attachments: N,
        roster: M,
        rules: R,
        postmortems: P,
        trail: A,
    ) -> Self {
        Self {
            readers,
            incidents,
            timeline,
            attachments,
            roster,
            rules,
            postmortems,
            trail,
        }
    }

    pub async fn get(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
    ) -> Result<Written, PostmortemError> {
        let grant = self.readers.grant(who, org).await?;
        self.visible(&grant, org, id).await?;
        let held = self
            .postmortems
            .of(org, id)
            .await?
            .ok_or(PostmortemError::NoSuchPostmortem)?;
        self.written(org, held).await
    }

    pub async fn summaries(
        &self,
        who: &Reader,
        org: TenantId,
    ) -> Result<Vec<PostmortemSummary>, PostmortemError> {
        let grant = self.readers.grant(who, org).await?;
        let held = self.postmortems.summaries(org).await?;
        let Some(project) = grant.project else {
            return Ok(held);
        };
        let mut seen = Vec::new();
        for summary in held {
            let incident = self.incidents.get(org, summary.incident).await?;
            if incident.is_some_and(|incident| incident.project == Some(project)) {
                seen.push(summary);
            }
        }
        Ok(seen)
    }

    pub async fn draft(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        force: bool,
        now: UnixSeconds,
    ) -> Result<Written, PostmortemError> {
        let (by, role) = self.writer(who, org).await?;
        if force && !role.at_least(OrgRole::Admin) {
            return Err(PostmortemError::NotAdmin);
        }
        let incident = self.incident(org, id).await?;
        let held = self.postmortems.of(org, id).await?;
        if let Some(held) = &held {
            if held.status == PostmortemStatus::Published {
                return Err(PostmortemError::Published);
            }
            if held.edited && !force {
                return Err(PostmortemError::Edited);
            }
        }
        let chronicle = self.chronicle(org, id).await?;
        let attachments = self.attachments.of(org, id).await?;
        let body = PostmortemBody::try_from(draft(&incident, &chronicle, &attachments).as_str())?;
        let now_ms = millis(now);
        let regenerated = held.is_some();
        let fresh = if let Some(held) = held {
            let fresh = Postmortem {
                body,
                author: Some(by),
                edited: false,
                updated_ms: now_ms,
                ..held
            };
            if !self.postmortems.keep(&fresh).await? {
                return Err(PostmortemError::NoSuchPostmortem);
            }
            fresh
        } else {
            let fresh = Postmortem {
                id: PostmortemId::fresh(),
                org,
                incident: id,
                status: PostmortemStatus::Draft,
                body,
                author: Some(by),
                edited: false,
                created_ms: now_ms,
                updated_ms: now_ms,
                published_ms: None,
            };
            if !self.postmortems.create(&fresh).await? {
                return Err(PostmortemError::Exists);
            }
            fresh
        };
        self.note(
            &fresh,
            by,
            Action::PostmortemDrafted,
            Detail::default().with("regenerated", regenerated),
        )
        .await?;
        self.written(org, fresh).await
    }

    pub async fn edit(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        body: &PostmortemBody,
        now: UnixSeconds,
    ) -> Result<Written, PostmortemError> {
        let (by, _) = self.writer(who, org).await?;
        let held = self.held(org, id).await?;
        if held.status == PostmortemStatus::Published {
            return Err(PostmortemError::Published);
        }
        let changed = Postmortem {
            body: body.clone(),
            edited: true,
            updated_ms: millis(now),
            ..held
        };
        self.kept(&changed).await?;
        self.note(
            &changed,
            by,
            Action::PostmortemEdited,
            Detail::default().with("bytes", body.as_str().len()),
        )
        .await?;
        self.written(org, changed).await
    }

    pub async fn publish(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        now: UnixSeconds,
    ) -> Result<Written, PostmortemError> {
        let (by, role) = self.writer(who, org).await?;
        if !role.at_least(OrgRole::Admin) {
            return Err(PostmortemError::NotAdmin);
        }
        let held = self.held(org, id).await?;
        if held.status == PostmortemStatus::Published {
            return self.written(org, held).await;
        }
        let now_ms = millis(now);
        let changed = Postmortem {
            status: PostmortemStatus::Published,
            updated_ms: now_ms,
            published_ms: Some(now_ms),
            ..held
        };
        self.kept(&changed).await?;
        self.note(&changed, by, Action::PostmortemPublished, Detail::default())
            .await?;
        self.written(org, changed).await
    }

    pub async fn unpublish(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        now: UnixSeconds,
    ) -> Result<Written, PostmortemError> {
        let (by, role) = self.writer(who, org).await?;
        if !role.at_least(OrgRole::Admin) {
            return Err(PostmortemError::NotAdmin);
        }
        let held = self.held(org, id).await?;
        if held.status == PostmortemStatus::Draft {
            return self.written(org, held).await;
        }
        let changed = Postmortem {
            status: PostmortemStatus::Draft,
            updated_ms: millis(now),
            published_ms: None,
            ..held
        };
        self.kept(&changed).await?;
        self.note(
            &changed,
            by,
            Action::PostmortemUnpublished,
            Detail::default(),
        )
        .await?;
        self.written(org, changed).await
    }

    async fn writer(
        &self,
        who: &Reader,
        org: TenantId,
    ) -> Result<(AccountId, OrgRole), PostmortemError> {
        match who {
            Reader::Key(_) | Reader::Agent(_) => {
                self.readers.grant(who, org).await?;
                Err(PostmortemError::ReadOnly)
            }
            Reader::Session(token) => {
                let who = self.readers.whoami().of(token).await?;
                who.role_in(org)
                    .map(|role| (who.account, role))
                    .ok_or(PostmortemError::NotYours)
            }
        }
    }

    async fn incident(&self, org: TenantId, id: IncidentId) -> Result<Incident, PostmortemError> {
        self.incidents
            .get(org, id)
            .await?
            .ok_or(PostmortemError::NoSuchIncident)
    }

    async fn visible(
        &self,
        grant: &ReadGrant,
        org: TenantId,
        id: IncidentId,
    ) -> Result<Incident, PostmortemError> {
        self.incident(org, id).await.and_then(|incident| {
            if grant.project.is_none() || incident.project == grant.project {
                Ok(incident)
            } else {
                Err(PostmortemError::NoSuchIncident)
            }
        })
    }

    async fn held(&self, org: TenantId, id: IncidentId) -> Result<Postmortem, PostmortemError> {
        self.incident(org, id).await?;
        self.postmortems
            .of(org, id)
            .await?
            .ok_or(PostmortemError::NoSuchPostmortem)
    }

    async fn kept(&self, postmortem: &Postmortem) -> Result<(), PostmortemError> {
        if self.postmortems.keep(postmortem).await? {
            Ok(())
        } else {
            Err(PostmortemError::NoSuchPostmortem)
        }
    }

    async fn chronicle(&self, org: TenantId, id: IncidentId) -> Result<Chronicle, PostmortemError> {
        Ok(Chronicle {
            entries: self.timeline.of(org, id).await?,
            people: self.people(org).await?,
            alerts: self
                .rules
                .list(org)
                .await?
                .into_iter()
                .map(|rule| AlertRef {
                    id: rule.id,
                    name: rule.name.as_str().into(),
                })
                .collect(),
        })
    }

    async fn people(&self, org: TenantId) -> Result<Vec<Person>, PostmortemError> {
        Ok(self
            .roster
            .of(org)
            .await?
            .into_iter()
            .map(|member| Person {
                account: member.account,
                email: Some(member.email),
                name: member.name,
            })
            .collect())
    }

    async fn written(
        &self,
        org: TenantId,
        postmortem: Postmortem,
    ) -> Result<Written, PostmortemError> {
        let author = match postmortem.author {
            None => None,
            Some(account) => Some(
                self.people(org)
                    .await?
                    .into_iter()
                    .find(|person| person.account == account)
                    .unwrap_or(Person {
                        account,
                        email: None,
                        name: None,
                    }),
            ),
        };
        Ok(Written { postmortem, author })
    }

    async fn note(
        &self,
        postmortem: &Postmortem,
        by: AccountId,
        action: Action,
        detail: Detail,
    ) -> Result<(), PostmortemError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org: postmortem.org,
                actor: Actor::Account(by),
                action,
                target: Target::of(
                    TargetKind::Postmortem,
                    Some(postmortem.id.to_string().into()),
                ),
                detail: detail.with("incident", postmortem.incident),
            })
            .await?)
    }
}

fn millis(now: UnixSeconds) -> u64 {
    now.get().saturating_mul(MILLIS_PER_SECOND)
}
