use core::future::Future;

use ferristrace_domain::{
    AccountId, Clock, OnCallNow, OncallError, Override, OverrideId, Page, PageDesk, PageId,
    PageScope, PageView, Paged, Pager, Paging, Reader, Readers, Rostered, Rota, ScheduleDraft,
    ScheduleId, SchedulePatch, SessionToken, TeamId, TenantId, Whoami,
};

use crate::registry::OncallRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: OncallRepos,
{
    #[transactional(sessions, memberships, api_keys, teams, roster, schedules, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn team_schedules(
        &self,
        who: &Reader,
        org: TenantId,
        team: TeamId,
    ) -> Result<Vec<Rostered>, OncallError> {
        Rota::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            teams,
            roster,
            schedules,
            audit,
        )
        .list(who, org, team, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, teams, roster, schedules, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn schedule(
        &self,
        who: &Reader,
        org: TenantId,
        id: ScheduleId,
    ) -> Result<Rostered, OncallError> {
        Rota::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            teams,
            roster,
            schedules,
            audit,
        )
        .get(who, org, id, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, teams, roster, schedules, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn on_call_now(
        &self,
        who: &Reader,
        org: TenantId,
        id: ScheduleId,
    ) -> Result<OnCallNow, OncallError> {
        Rota::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            teams,
            roster,
            schedules,
            audit,
        )
        .now(who, org, id, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, teams, roster, schedules, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn create_schedule(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        draft: &ScheduleDraft,
    ) -> Result<Rostered, OncallError> {
        Rota::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            teams,
            roster,
            schedules,
            audit,
        )
        .create(token, org, team, draft, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, teams, roster, schedules, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn update_schedule(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ScheduleId,
        patch: SchedulePatch,
    ) -> Result<Rostered, OncallError> {
        Rota::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            teams,
            roster,
            schedules,
            audit,
        )
        .update(token, org, id, patch, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, teams, roster, schedules, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn delete_schedule(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ScheduleId,
    ) -> Result<(), OncallError> {
        Rota::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            teams,
            roster,
            schedules,
            audit,
        )
        .delete(token, org, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, teams, roster, schedules, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn add_override(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ScheduleId,
        account: AccountId,
        from_ms: u64,
        to_ms: u64,
    ) -> Result<Override, OncallError> {
        Rota::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            teams,
            roster,
            schedules,
            audit,
        )
        .add_override(token, org, id, account, from_ms, to_ms)
        .await
    }

    #[transactional(sessions, memberships, api_keys, teams, roster, schedules, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn remove_override(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ScheduleId,
        cover: OverrideId,
    ) -> Result<(), OncallError> {
        Rota::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            teams,
            roster,
            schedules,
            audit,
        )
        .remove_override(token, org, id, cover)
        .await
    }

    #[transactional(sessions, memberships, api_keys, roster, schedules, pages, incident_timeline, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn list_pages(
        &self,
        who: &Reader,
        org: TenantId,
        scope: PageScope,
    ) -> Result<Vec<PageView>, OncallError> {
        PageDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            roster,
            schedules,
            pages,
            incident_timeline,
            audit,
        )
        .list(who, org, scope)
        .await
    }

    #[transactional(sessions, memberships, api_keys, roster, schedules, pages, incident_timeline, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn acknowledge_page(
        &self,
        who: &Reader,
        org: TenantId,
        id: PageId,
    ) -> Result<PageView, OncallError> {
        PageDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            roster,
            schedules,
            pages,
            incident_timeline,
            audit,
        )
        .acknowledge(who, org, id, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, roster, schedules, pages, incident_timeline, audit, unavailable = OncallError::StoreUnreachable)]
    pub async fn acknowledge_page_link(&self, token: &str) -> Result<Page, OncallError> {
        PageDesk::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            roster,
            schedules,
            pages,
            incident_timeline,
            audit,
        )
        .acknowledge_link(token, self.ack_key.as_ref(), self.clock.now())
        .await
    }

    #[allow(clippy::manual_async_fn)]
    pub fn escalate_pages(&self) -> impl Future<Output = Result<Paged, OncallError>> + Send {
        async move { self.escalate_once().await }
    }

    #[transactional(schedules, pages, alert_duty, roster, incident_timeline, audit, unavailable = OncallError::StoreUnreachable)]
    async fn escalate_once(&self) -> Result<Paged, OncallError> {
        Pager::new(
            Paging::new(schedules, pages).escalating_after(self.escalate_after_secs),
            alert_duty,
            roster,
            incident_timeline,
            audit,
            &self.mailer,
            self.ack_key.as_ref(),
        )
        .tick(self.clock.now())
        .await
    }
}
