use ferristrace_domain::{
    AlertError, Attachment, AttachmentLabel, BadIncident, Briefed, Chronicle, Clock, CommentBody,
    DEFAULT_WINDOW_DAYS, Dossier, Drained, IncidentContext, IncidentDesk, IncidentDraft,
    IncidentError, IncidentFilter, IncidentId, IncidentInsights, IncidentPatch, MAX_WINDOW_DAYS,
    Narrator, Paging, ProjectId, Reader, Readers, Reference, Surroundings, TenantId, Whoami,
};

use crate::registry::{IncidentRepos, NarrateRepos};
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: IncidentRepos,
{
    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, schedules, pages, status_outbox, unavailable = IncidentError::StoreUnreachable)]
    pub async fn list_incidents(
        &self,
        who: &Reader,
        org: TenantId,
        filter: IncidentFilter,
    ) -> Result<Vec<Briefed>, IncidentError> {
        IncidentDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            audit,
            Paging::new(schedules, pages).escalating_after(self.escalate_after_secs),
            status_outbox,
        )
        .list(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, schedules, pages, status_outbox, unavailable = IncidentError::StoreUnreachable)]
    pub async fn incident_insights(
        &self,
        who: &Reader,
        org: TenantId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        project: Option<ProjectId>,
    ) -> Result<IncidentInsights, IncidentError> {
        const DAY_MS: u64 = 86_400_000;
        let to_ms = to_ms.unwrap_or_else(|| self.clock.now().get().saturating_mul(1_000));
        let from_ms = from_ms.unwrap_or_else(|| to_ms.saturating_sub(DEFAULT_WINDOW_DAYS * DAY_MS));
        if from_ms >= to_ms || to_ms - from_ms > MAX_WINDOW_DAYS * DAY_MS {
            return Err(BadIncident::Window {
                max: MAX_WINDOW_DAYS,
            }
            .into());
        }
        IncidentDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            audit,
            Paging::new(schedules, pages).escalating_after(self.escalate_after_secs),
            status_outbox,
        )
        .insights(who, org, from_ms, to_ms, project)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, schedules, pages, status_outbox, unavailable = IncidentError::StoreUnreachable)]
    pub async fn incident(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
    ) -> Result<Dossier, IncidentError> {
        IncidentDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            audit,
            Paging::new(schedules, pages).escalating_after(self.escalate_after_secs),
            status_outbox,
        )
        .get(who, org, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, schedules, pages, status_outbox, unavailable = IncidentError::StoreUnreachable)]
    pub async fn incident_timeline(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
    ) -> Result<Chronicle, IncidentError> {
        IncidentDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            audit,
            Paging::new(schedules, pages).escalating_after(self.escalate_after_secs),
            status_outbox,
        )
        .timeline(who, org, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, schedules, pages, status_outbox, unavailable = IncidentError::StoreUnreachable)]
    pub async fn open_incident(
        &self,
        who: &Reader,
        org: TenantId,
        draft: &IncidentDraft,
    ) -> Result<Briefed, IncidentError> {
        IncidentDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            audit,
            Paging::new(schedules, pages).escalating_after(self.escalate_after_secs),
            status_outbox,
        )
        .open(who, org, draft, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, schedules, pages, status_outbox, unavailable = IncidentError::StoreUnreachable)]
    pub async fn amend_incident(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        patch: IncidentPatch,
    ) -> Result<Briefed, IncidentError> {
        IncidentDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            audit,
            Paging::new(schedules, pages).escalating_after(self.escalate_after_secs),
            status_outbox,
        )
        .amend(who, org, id, patch, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, schedules, pages, status_outbox, unavailable = IncidentError::StoreUnreachable)]
    pub async fn comment_on_incident(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        body: &CommentBody,
        public: bool,
    ) -> Result<Chronicle, IncidentError> {
        IncidentDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            audit,
            Paging::new(schedules, pages).escalating_after(self.escalate_after_secs),
            status_outbox,
        )
        .comment(who, org, id, body, public, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, schedules, pages, status_outbox, unavailable = IncidentError::StoreUnreachable)]
    pub async fn attach_to_incident(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        reference: Reference,
        label: AttachmentLabel,
    ) -> Result<Attachment, IncidentError> {
        IncidentDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            audit,
            Paging::new(schedules, pages).escalating_after(self.escalate_after_secs),
            status_outbox,
        )
        .attach(who, org, id, reference, label, self.clock.now())
        .await
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: IncidentRepos,
{
    pub async fn incident_context(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
    ) -> Result<IncidentContext, IncidentError> {
        let dossier = self.incident(who, org, id).await?;
        let (Some(issues), Some(spans), Some(infra)) =
            (&self.issue_store, &self.span_store, &self.metric_store)
        else {
            return Err(IncidentError::StoreUnreachable);
        };
        let services = dossier
            .briefed
            .alert
            .as_ref()
            .map(|alert| alert.services.clone())
            .unwrap_or_default();
        Ok(Surroundings::new(issues, issues, spans, infra)
            .around(
                &dossier.briefed.incident,
                &services,
                self.clock.now().get().saturating_mul(1_000),
            )
            .await?)
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: NarrateRepos,
{
    #[allow(clippy::manual_async_fn)]
    pub fn narrate_incidents(
        &self,
    ) -> impl core::future::Future<Output = Result<Drained, AlertError>> + Send {
        async move { self.narrate_once().await }
    }

    #[transactional(incident_outbox, alert_duty, unavailable = AlertError::StoreUnreachable)]
    async fn narrate_once(&self) -> Result<Drained, AlertError> {
        Narrator::new(incident_outbox, alert_duty, &self.webhooks)
            .tick(self.clock.now())
            .await
    }
}
