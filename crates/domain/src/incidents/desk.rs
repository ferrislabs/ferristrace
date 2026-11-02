use super::entities::{
    AlertRef, Attachment, Author, Briefed, Change, Chronicle, Dossier, Incident, IncidentDraft,
    IncidentFilter, IncidentPatch, Person, TimelineEntry,
};
use super::errors::{BadIncident, IncidentError};
use super::ports::{IncidentAttachments, IncidentTimeline, Incidents};
use super::value_objects::{
    AttachmentId, AttachmentLabel, CommentBody, IncidentId, IncidentSeverity, IncidentStatus,
    IncidentTitle, Reference, TimelineId,
};
use crate::access::{ApiKeys, ReadGrant, Reader, Readers};
use crate::alerts::{AlertRules, AlertStates, AlertStatus, Rule};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::catalogue::{ProjectId, Projects};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, SessionStore};
use crate::oncall::{Origin, PageSource, Pages, Paging, Schedules};
use crate::organisation::{Memberships, Roll};
use crate::telemetry::TenantId;

const MILLIS_PER_SECOND: u64 = 1_000;
pub const DEFAULT_SEVERITY: IncidentSeverity = IncidentSeverity::Sev3;

#[derive(Debug)]
pub struct IncidentDesk<S, G, K, P, R, T, I, L, N, M, A, Sc, Pg> {
    readers: Readers<S, G, K>,
    projects: P,
    rules: R,
    states: T,
    incidents: I,
    timeline: L,
    attachments: N,
    roster: M,
    trail: A,
    paging: Paging<Sc, Pg>,
}

impl<S, G, K, P, R, T, I, L, N, M, A, Sc, Pg> IncidentDesk<S, G, K, P, R, T, I, L, N, M, A, Sc, Pg>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    P: Projects,
    R: AlertRules,
    T: AlertStates,
    I: Incidents,
    L: IncidentTimeline,
    N: IncidentAttachments,
    M: Roll,
    A: AuditTrail,
    Sc: Schedules,
    Pg: Pages,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        readers: Readers<S, G, K>,
        projects: P,
        rules: R,
        states: T,
        incidents: I,
        timeline: L,
        attachments: N,
        roster: M,
        trail: A,
        paging: Paging<Sc, Pg>,
    ) -> Self {
        Self {
            readers,
            projects,
            rules,
            states,
            incidents,
            timeline,
            attachments,
            roster,
            trail,
            paging,
        }
    }

    pub async fn list(
        &self,
        who: &Reader,
        org: TenantId,
        filter: IncidentFilter,
    ) -> Result<Vec<Briefed>, IncidentError> {
        let grant = self.readers.grant(who, org).await?;
        let project = grant.narrow(filter.project)?;
        let held = self
            .incidents
            .list(
                org,
                &IncidentFilter {
                    scope: filter.scope,
                    project,
                },
            )
            .await?;
        let people = self.people(org).await?;
        let alerts = self.alerts(org).await?;
        Ok(held
            .into_iter()
            .filter(|incident| visible(&grant, incident))
            .map(|incident| brief(incident, &people, &alerts))
            .collect())
    }

    pub async fn get(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
    ) -> Result<Dossier, IncidentError> {
        let grant = self.readers.grant(who, org).await?;
        let incident = self.visible_incident(&grant, org, id).await?;
        let attachments = self.attachments.of(org, id).await?;
        let chronicle = self.chronicle(org, id).await?;
        Ok(Dossier {
            briefed: brief(incident, &chronicle.people, &chronicle.alerts),
            attachments,
            chronicle,
        })
    }

    pub async fn timeline(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
    ) -> Result<Chronicle, IncidentError> {
        let grant = self.readers.grant(who, org).await?;
        self.visible_incident(&grant, org, id).await?;
        self.chronicle(org, id).await
    }

    pub async fn open(
        &self,
        who: &Reader,
        org: TenantId,
        draft: &IncidentDraft,
        now: UnixSeconds,
    ) -> Result<Briefed, IncidentError> {
        let by = self.writer(who, org).await?;
        let rule = match draft.alert {
            None => None,
            Some(alert) => Some(self.firing_rule(org, alert).await?),
        };
        let project = draft
            .project
            .or_else(|| rule.as_ref().and_then(|rule| rule.project));
        self.check_project(org, project).await?;
        if let Some(schedule) = draft.page
            && self.paging.schedules().get(org, schedule).await?.is_none()
        {
            return Err(IncidentError::NoSuchSchedule);
        }
        let title = match (&draft.title, &rule) {
            (Some(title), _) => title.clone(),
            (None, Some(rule)) => IncidentTitle::try_from(rule.name.as_str())?,
            (None, None) => {
                return Err(BadIncident::Title {
                    max: super::value_objects::MAX_TITLE_BYTES,
                }
                .into());
            }
        };
        let severity = draft.severity.unwrap_or_else(|| {
            rule.as_ref().map_or(DEFAULT_SEVERITY, |rule| {
                IncidentSeverity::from(rule.definition.severity())
            })
        });
        let now_ms = millis(now);
        let incident = Incident {
            id: IncidentId::fresh(),
            org,
            project,
            title,
            severity,
            status: IncidentStatus::Investigating,
            owner: None,
            public: draft.public,
            opened_ms: now_ms,
            resolved_ms: None,
            opened_from_alert: rule.as_ref().map(|rule| rule.id),
            paged_schedule: draft.page,
        };
        self.incidents.open(&incident).await?;
        self.append(
            &incident,
            by,
            Change::Opened {
                title: incident.title.clone(),
                severity,
                alert: incident.opened_from_alert,
            },
            now_ms,
        )
        .await?;
        self.note(
            &incident,
            by,
            Action::IncidentOpened,
            Detail::default()
                .with("title", &incident.title)
                .with("severity", severity)
                .with_maybe("alert", incident.opened_from_alert)
                .with_maybe("paged", incident.paged_schedule),
        )
        .await?;
        self.page_on_call(&incident, now_ms).await?;
        let people = self.people(org).await?;
        let alerts = rule
            .map(|rule| AlertRef {
                id: rule.id,
                name: rule.name.as_str().into(),
            })
            .into_iter()
            .collect::<Vec<_>>();
        Ok(brief(incident, &people, &alerts))
    }

    pub async fn amend(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        patch: IncidentPatch,
        now: UnixSeconds,
    ) -> Result<Briefed, IncidentError> {
        let by = self.writer(who, org).await?;
        let held = self
            .incidents
            .get(org, id)
            .await?
            .ok_or(IncidentError::NoSuchIncident)?;
        let people = self.people(org).await?;
        if let Some(Some(owner)) = patch.owner
            && !people.iter().any(|person| person.account == owner)
        {
            return Err(IncidentError::NotAMember);
        }
        let now_ms = millis(now);
        let (changed, steps) = amended(&held, patch, now_ms);
        if steps.is_empty() {
            let alerts = self.alerts(org).await?;
            return Ok(brief(held, &people, &alerts));
        }
        if !self.incidents.keep(&changed).await? {
            return Err(IncidentError::NoSuchIncident);
        }
        let mut updated = Vec::new();
        for change in steps {
            match &change {
                Change::Status { from, to } if from.is_open() && !to.is_open() => {
                    self.note(&changed, by, Action::IncidentResolved, Detail::default())
                        .await?;
                    self.paging
                        .resolve(org, Origin::Incident(id), now_ms)
                        .await?;
                }
                Change::Status { from, to } if !from.is_open() && to.is_open() => {
                    self.note(
                        &changed,
                        by,
                        Action::IncidentReopened,
                        Detail::default().with("status", to),
                    )
                    .await?;
                }
                other => updated.push(other.kind().as_str()),
            }
            self.append(&changed, by, change, now_ms).await?;
        }
        if !updated.is_empty() {
            self.note(
                &changed,
                by,
                Action::IncidentUpdated,
                Detail::default().with_all("fields", &updated),
            )
            .await?;
        }
        let alerts = self.alerts(org).await?;
        Ok(brief(changed, &people, &alerts))
    }

    pub async fn comment(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        body: &CommentBody,
        now: UnixSeconds,
    ) -> Result<Chronicle, IncidentError> {
        let by = self.writer(who, org).await?;
        let incident = self.writable(org, id).await?;
        let entry = self
            .append(
                &incident,
                by,
                Change::Comment { body: body.clone() },
                millis(now),
            )
            .await?;
        self.note(
            &incident,
            by,
            Action::IncidentCommented,
            Detail::default().with("bytes", body.as_str().len()),
        )
        .await?;
        Ok(Chronicle {
            entries: vec![entry],
            people: self.people(org).await?,
            alerts: Vec::new(),
        })
    }

    pub async fn attach(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        reference: Reference,
        label: AttachmentLabel,
        now: UnixSeconds,
    ) -> Result<Attachment, IncidentError> {
        let by = self.writer(who, org).await?;
        let incident = self.writable(org, id).await?;
        let now_ms = millis(now);
        let attachment = Attachment {
            id: AttachmentId::fresh(),
            incident: id,
            reference,
            label,
            at_ms: now_ms,
        };
        self.attachments.attach(org, &attachment).await?;
        self.append(
            &incident,
            by,
            Change::Attachment(attachment.clone()),
            now_ms,
        )
        .await?;
        self.note(
            &incident,
            by,
            Action::IncidentAttached,
            Detail::default()
                .with("kind", attachment.kind())
                .with("label", attachment.label.as_str()),
        )
        .await?;
        Ok(attachment)
    }

    async fn page_on_call(&self, incident: &Incident, now_ms: u64) -> Result<(), IncidentError> {
        let Some(schedule) = incident.paged_schedule else {
            return Ok(());
        };
        let Some(page) = self
            .paging
            .raise(
                incident.org,
                schedule,
                PageSource::Incident(incident.id),
                incident.title.as_str(),
                now_ms,
            )
            .await?
        else {
            return Ok(());
        };
        self.timeline
            .append(&TimelineEntry {
                id: TimelineId::fresh(),
                incident: incident.id,
                org: incident.org,
                author: Author::System,
                change: Change::Paged {
                    page: page.id,
                    step: page.step,
                    target: Some(page.target),
                },
                at_ms: now_ms,
            })
            .await?;
        Ok(())
    }

    async fn writer(&self, who: &Reader, org: TenantId) -> Result<AccountId, IncidentError> {
        match who {
            Reader::Key(_) => {
                self.readers.grant(who, org).await?;
                Err(IncidentError::ReadOnly)
            }
            Reader::Session(token) => {
                let who = self.readers.whoami().of(token).await?;
                who.role_in(org)
                    .map(|_| who.account)
                    .ok_or(IncidentError::NotYours)
            }
        }
    }

    async fn writable(&self, org: TenantId, id: IncidentId) -> Result<Incident, IncidentError> {
        self.incidents
            .get(org, id)
            .await?
            .ok_or(IncidentError::NoSuchIncident)
    }

    async fn visible_incident(
        &self,
        grant: &ReadGrant,
        org: TenantId,
        id: IncidentId,
    ) -> Result<Incident, IncidentError> {
        self.incidents
            .get(org, id)
            .await?
            .filter(|incident| visible(grant, incident))
            .ok_or(IncidentError::NoSuchIncident)
    }

    async fn firing_rule(
        &self,
        org: TenantId,
        id: crate::alerts::RuleId,
    ) -> Result<Rule, IncidentError> {
        let rule = self
            .rules
            .get(org, id)
            .await?
            .ok_or(IncidentError::NoSuchAlert)?;
        let firing = self
            .states
            .of(rule.id)
            .await?
            .iter()
            .any(|series| series.state.status == AlertStatus::Firing);
        if firing {
            Ok(rule)
        } else {
            Err(IncidentError::AlertNotFiring)
        }
    }

    async fn check_project(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
    ) -> Result<(), IncidentError> {
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
            Err(IncidentError::NoSuchProject)
        }
    }

    async fn people(&self, org: TenantId) -> Result<Vec<Person>, IncidentError> {
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

    async fn alerts(&self, org: TenantId) -> Result<Vec<AlertRef>, IncidentError> {
        Ok(self
            .rules
            .list(org)
            .await?
            .into_iter()
            .map(|rule| AlertRef {
                id: rule.id,
                name: rule.name.as_str().into(),
            })
            .collect())
    }

    async fn chronicle(&self, org: TenantId, id: IncidentId) -> Result<Chronicle, IncidentError> {
        Ok(Chronicle {
            entries: self.timeline.of(org, id).await?,
            people: self.people(org).await?,
            alerts: self.alerts(org).await?,
        })
    }

    async fn append(
        &self,
        incident: &Incident,
        by: AccountId,
        change: Change,
        at_ms: u64,
    ) -> Result<TimelineEntry, IncidentError> {
        let entry = TimelineEntry {
            id: TimelineId::fresh(),
            incident: incident.id,
            org: incident.org,
            author: Author::Account(by),
            change,
            at_ms,
        };
        self.timeline.append(&entry).await?;
        Ok(entry)
    }

    async fn note(
        &self,
        incident: &Incident,
        by: AccountId,
        action: Action,
        detail: Detail,
    ) -> Result<(), IncidentError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org: incident.org,
                actor: Actor::Account(by),
                action,
                target: Target::of(TargetKind::Incident, Some(incident.id.to_string().into())),
                detail,
            })
            .await?)
    }
}

fn millis(now: UnixSeconds) -> u64 {
    now.get().saturating_mul(MILLIS_PER_SECOND)
}

fn visible(grant: &ReadGrant, incident: &Incident) -> bool {
    grant.project.is_none() || incident.project == grant.project
}

fn brief(incident: Incident, people: &[Person], alerts: &[AlertRef]) -> Briefed {
    let owner = incident.owner.map(|owner| {
        people
            .iter()
            .find(|person| person.account == owner)
            .cloned()
            .unwrap_or(Person {
                account: owner,
                email: None,
                name: None,
            })
    });
    let alert = incident
        .opened_from_alert
        .and_then(|rule| alerts.iter().find(|alert| alert.id == rule).cloned());
    Briefed {
        incident,
        owner,
        alert,
    }
}

fn amended(held: &Incident, patch: IncidentPatch, now_ms: u64) -> (Incident, Vec<Change>) {
    let mut changed = held.clone();
    let mut steps = Vec::new();
    if let Some(title) = patch.title
        && title != held.title
    {
        steps.push(Change::Title {
            from: held.title.clone(),
            to: title.clone(),
        });
        changed.title = title;
    }
    if let Some(severity) = patch.severity
        && severity != held.severity
    {
        steps.push(Change::Severity {
            from: held.severity,
            to: severity,
        });
        changed.severity = severity;
    }
    if let Some(status) = patch.status
        && status != held.status
    {
        steps.push(Change::Status {
            from: held.status,
            to: status,
        });
        changed.status = status;
        changed.resolved_ms = if status.is_open() { None } else { Some(now_ms) };
    }
    if let Some(owner) = patch.owner
        && owner != held.owner
    {
        steps.push(Change::Owner {
            from: held.owner,
            to: owner,
        });
        changed.owner = owner;
    }
    if let Some(public) = patch.public
        && public != held.public
    {
        steps.push(Change::Public {
            from: held.public,
            to: public,
        });
        changed.public = public;
    }
    (changed, steps)
}
