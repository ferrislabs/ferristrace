use ferristrace_domain::{
    Attachment, AttachmentLabel, Briefed, Chronicle, Clock, CommentBody, Dossier, IncidentDesk,
    IncidentDraft, IncidentError, IncidentFilter, IncidentId, IncidentPatch, Reader, Readers,
    Reference, TenantId, Whoami,
};

use crate::registry::IncidentRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: IncidentRepos,
{
    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, unavailable = IncidentError::StoreUnreachable)]
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
        )
        .list(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, unavailable = IncidentError::StoreUnreachable)]
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
        )
        .get(who, org, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, unavailable = IncidentError::StoreUnreachable)]
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
        )
        .timeline(who, org, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, unavailable = IncidentError::StoreUnreachable)]
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
        )
        .open(who, org, draft, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, unavailable = IncidentError::StoreUnreachable)]
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
        )
        .amend(who, org, id, patch, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, unavailable = IncidentError::StoreUnreachable)]
    pub async fn comment_on_incident(
        &self,
        who: &Reader,
        org: TenantId,
        id: IncidentId,
        body: &CommentBody,
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
        )
        .comment(who, org, id, body, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, incidents, incident_timeline, incident_attachments, roster, audit, unavailable = IncidentError::StoreUnreachable)]
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
        )
        .attach(who, org, id, reference, label, self.clock.now())
        .await
    }
}
