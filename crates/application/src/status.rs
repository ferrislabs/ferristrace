use core::future::Future;

use ferristrace_domain::{
    Clock, ComponentDraft, ComponentId, ComponentPatch, Email, Herald, Heralded, IncidentId,
    Landing, Listed, PageDraft, PagePatch, PublicPostmortem, PublicView, Reader, Readers, Sheet,
    Showcase, Slug, StatusDesk, StatusError, StatusPageId, Subscriptions, TenantId, Whoami,
};

use crate::registry::StatusRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: StatusRepos,
{
    #[transactional(sessions, memberships, api_keys, projects, alert_rules, status_pages, status_subscribers, audit, unavailable = StatusError::StoreUnreachable)]
    pub async fn status_pages(
        &self,
        who: &Reader,
        org: TenantId,
    ) -> Result<Vec<Listed>, StatusError> {
        StatusDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            status_pages,
            status_subscribers,
            audit,
        )
        .list(who, org)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, status_pages, status_subscribers, audit, unavailable = StatusError::StoreUnreachable)]
    pub async fn status_page(
        &self,
        who: &Reader,
        org: TenantId,
        id: StatusPageId,
    ) -> Result<Sheet, StatusError> {
        StatusDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            status_pages,
            status_subscribers,
            audit,
        )
        .get(who, org, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, status_pages, status_subscribers, audit, unavailable = StatusError::StoreUnreachable)]
    pub async fn create_status_page(
        &self,
        who: &Reader,
        org: TenantId,
        draft: &PageDraft,
    ) -> Result<Sheet, StatusError> {
        StatusDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            status_pages,
            status_subscribers,
            audit,
        )
        .create(who, org, draft, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, status_pages, status_subscribers, audit, unavailable = StatusError::StoreUnreachable)]
    pub async fn update_status_page(
        &self,
        who: &Reader,
        org: TenantId,
        id: StatusPageId,
        patch: PagePatch,
    ) -> Result<Sheet, StatusError> {
        StatusDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            status_pages,
            status_subscribers,
            audit,
        )
        .update(who, org, id, patch, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, status_pages, status_subscribers, audit, unavailable = StatusError::StoreUnreachable)]
    pub async fn delete_status_page(
        &self,
        who: &Reader,
        org: TenantId,
        id: StatusPageId,
    ) -> Result<(), StatusError> {
        StatusDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            status_pages,
            status_subscribers,
            audit,
        )
        .delete(who, org, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, status_pages, status_subscribers, audit, unavailable = StatusError::StoreUnreachable)]
    pub async fn add_status_component(
        &self,
        who: &Reader,
        org: TenantId,
        page: StatusPageId,
        draft: &ComponentDraft,
    ) -> Result<ferristrace_domain::Component, StatusError> {
        StatusDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            status_pages,
            status_subscribers,
            audit,
        )
        .add_component(who, org, page, draft)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, status_pages, status_subscribers, audit, unavailable = StatusError::StoreUnreachable)]
    pub async fn change_status_component(
        &self,
        who: &Reader,
        org: TenantId,
        page: StatusPageId,
        id: ComponentId,
        patch: ComponentPatch,
    ) -> Result<ferristrace_domain::Component, StatusError> {
        StatusDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            status_pages,
            status_subscribers,
            audit,
        )
        .change_component(who, org, page, id, patch)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, status_pages, status_subscribers, audit, unavailable = StatusError::StoreUnreachable)]
    pub async fn remove_status_component(
        &self,
        who: &Reader,
        org: TenantId,
        page: StatusPageId,
        id: ComponentId,
    ) -> Result<(), StatusError> {
        StatusDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            status_pages,
            status_subscribers,
            audit,
        )
        .remove_component(who, org, page, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, status_pages, status_subscribers, audit, unavailable = StatusError::StoreUnreachable)]
    pub async fn order_status_components(
        &self,
        who: &Reader,
        org: TenantId,
        page: StatusPageId,
        ids: &[ComponentId],
    ) -> Result<Vec<ferristrace_domain::Component>, StatusError> {
        StatusDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            status_pages,
            status_subscribers,
            audit,
        )
        .order(who, org, page, ids)
        .await
    }

    #[transactional(sessions, memberships, api_keys, alert_rules, alert_states, status_pages, incidents, incident_timeline, postmortems, unavailable = StatusError::StoreUnreachable)]
    pub async fn preview_status_page(
        &self,
        who: &Reader,
        org: TenantId,
        id: StatusPageId,
    ) -> Result<PublicView, StatusError> {
        Showcase::new(
            status_pages,
            incidents,
            incident_timeline,
            postmortems,
            alert_rules,
            alert_states,
            self.span_store.as_ref(),
        )
        .preview(
            &Readers::new(Whoami::new(sessions, memberships), api_keys),
            who,
            org,
            id,
            self.clock.now(),
        )
        .await
    }

    #[transactional(alert_rules, alert_states, status_pages, incidents, incident_timeline, postmortems, unavailable = StatusError::StoreUnreachable)]
    pub async fn public_status(&self, slug: &Slug) -> Result<PublicView, StatusError> {
        Showcase::new(
            status_pages,
            incidents,
            incident_timeline,
            postmortems,
            alert_rules,
            alert_states,
            self.span_store.as_ref(),
        )
        .view(slug, self.clock.now())
        .await
    }

    #[transactional(alert_rules, alert_states, status_pages, incidents, incident_timeline, postmortems, unavailable = StatusError::StoreUnreachable)]
    pub async fn public_postmortem(
        &self,
        slug: &Slug,
        incident: IncidentId,
    ) -> Result<PublicPostmortem, StatusError> {
        Showcase::new(
            status_pages,
            incidents,
            incident_timeline,
            postmortems,
            alert_rules,
            alert_states,
            self.span_store.as_ref(),
        )
        .postmortem(slug, incident)
        .await
    }

    #[transactional(status_pages, status_subscribers, status_outbox, unavailable = StatusError::StoreUnreachable)]
    pub async fn subscribe_to_status(
        &self,
        slug: &Slug,
        email: &Email,
    ) -> Result<bool, StatusError> {
        Subscriptions::new(
            status_pages,
            status_subscribers,
            status_outbox,
            self.status_key.as_ref(),
        )
        .subscribe(slug, email, self.clock.now())
        .await
    }

    #[transactional(status_pages, status_subscribers, status_outbox, unavailable = StatusError::StoreUnreachable)]
    pub async fn confirm_status_subscription(&self, token: &str) -> Result<Landing, StatusError> {
        Subscriptions::new(
            status_pages,
            status_subscribers,
            status_outbox,
            self.status_key.as_ref(),
        )
        .confirm(token, self.clock.now())
        .await
    }

    #[transactional(status_pages, status_subscribers, status_outbox, unavailable = StatusError::StoreUnreachable)]
    pub async fn unsubscribe_from_status(&self, token: &str) -> Result<Landing, StatusError> {
        Subscriptions::new(
            status_pages,
            status_subscribers,
            status_outbox,
            self.status_key.as_ref(),
        )
        .unsubscribe(token, self.clock.now())
        .await
    }

    #[allow(clippy::manual_async_fn)]
    pub fn announce_status(&self) -> impl Future<Output = Result<Heralded, StatusError>> + Send {
        async move { self.announce_once().await }
    }

    #[transactional(status_outbox, alert_duty, unavailable = StatusError::StoreUnreachable)]
    async fn announce_once(&self) -> Result<Heralded, StatusError> {
        Herald::new(
            status_outbox,
            alert_duty,
            &self.mailer,
            self.status_key.as_ref(),
        )
        .tick(self.clock.now())
        .await
    }
}
