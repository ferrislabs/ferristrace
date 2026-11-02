use ferristrace_domain::{
    Clock, IncidentId, PostmortemBody, PostmortemDesk, PostmortemError, PostmortemSummary, Reader,
    Readers, TenantId, Whoami, Written,
};

use crate::registry::PostmortemRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: PostmortemRepos,
{
    #[transactional(sessions, memberships, api_keys, incidents, incident_timeline, incident_attachments, roster, alert_rules, postmortems, audit, unavailable = PostmortemError::StoreUnreachable)]
    pub async fn postmortem(
        &self,
        who: &Reader,
        org: TenantId,
        incident: IncidentId,
    ) -> Result<Written, PostmortemError> {
        PostmortemDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            alert_rules,
            postmortems,
            audit,
        )
        .get(who, org, incident)
        .await
    }

    #[transactional(sessions, memberships, api_keys, incidents, incident_timeline, incident_attachments, roster, alert_rules, postmortems, audit, unavailable = PostmortemError::StoreUnreachable)]
    pub async fn postmortem_summaries(
        &self,
        who: &Reader,
        org: TenantId,
    ) -> Result<Vec<PostmortemSummary>, PostmortemError> {
        PostmortemDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            alert_rules,
            postmortems,
            audit,
        )
        .summaries(who, org)
        .await
    }

    #[transactional(sessions, memberships, api_keys, incidents, incident_timeline, incident_attachments, roster, alert_rules, postmortems, audit, unavailable = PostmortemError::StoreUnreachable)]
    pub async fn draft_postmortem(
        &self,
        who: &Reader,
        org: TenantId,
        incident: IncidentId,
        force: bool,
    ) -> Result<Written, PostmortemError> {
        PostmortemDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            alert_rules,
            postmortems,
            audit,
        )
        .draft(who, org, incident, force, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, incidents, incident_timeline, incident_attachments, roster, alert_rules, postmortems, audit, unavailable = PostmortemError::StoreUnreachable)]
    pub async fn edit_postmortem(
        &self,
        who: &Reader,
        org: TenantId,
        incident: IncidentId,
        body: &PostmortemBody,
    ) -> Result<Written, PostmortemError> {
        PostmortemDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            alert_rules,
            postmortems,
            audit,
        )
        .edit(who, org, incident, body, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, incidents, incident_timeline, incident_attachments, roster, alert_rules, postmortems, audit, unavailable = PostmortemError::StoreUnreachable)]
    pub async fn publish_postmortem(
        &self,
        who: &Reader,
        org: TenantId,
        incident: IncidentId,
    ) -> Result<Written, PostmortemError> {
        PostmortemDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            alert_rules,
            postmortems,
            audit,
        )
        .publish(who, org, incident, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, incidents, incident_timeline, incident_attachments, roster, alert_rules, postmortems, audit, unavailable = PostmortemError::StoreUnreachable)]
    pub async fn unpublish_postmortem(
        &self,
        who: &Reader,
        org: TenantId,
        incident: IncidentId,
    ) -> Result<Written, PostmortemError> {
        PostmortemDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            incidents,
            incident_timeline,
            incident_attachments,
            roster,
            alert_rules,
            postmortems,
            audit,
        )
        .unpublish(who, org, incident, self.clock.now())
        .await
    }
}
