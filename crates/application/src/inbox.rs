use ferristrace_domain::{
    AccountId, Clock, InboxDesk, InboxError, InboxItem, InboxItemDraft, InboxItemKind,
    NotificationPreferences, SessionToken, TenantId, Whoami,
};

use crate::registry::InboxRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: InboxRepos,
{
    #[transactional(sessions, memberships, inbox, unavailable = InboxError::StoreUnreachable)]
    pub async fn notifications(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<InboxItem>, InboxError> {
        InboxDesk::new(Whoami::new(sessions, memberships), inbox.clone(), inbox)
            .list(token, org)
            .await
    }

    #[transactional(sessions, memberships, inbox, unavailable = InboxError::StoreUnreachable)]
    pub async fn mark_notification_read(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ferristrace_domain::InboxItemId,
    ) -> Result<(), InboxError> {
        InboxDesk::new(Whoami::new(sessions, memberships), inbox.clone(), inbox)
            .mark_read(token, org, id, self.clock.now().get() * 1_000)
            .await
    }

    #[transactional(sessions, memberships, inbox, unavailable = InboxError::StoreUnreachable)]
    pub async fn mark_all_notifications_read(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<u64, InboxError> {
        InboxDesk::new(Whoami::new(sessions, memberships), inbox.clone(), inbox)
            .mark_all_read(token, org, self.clock.now().get() * 1_000)
            .await
    }

    #[transactional(sessions, memberships, inbox, unavailable = InboxError::StoreUnreachable)]
    pub async fn notification_preferences(
        &self,
        token: &SessionToken,
    ) -> Result<NotificationPreferences, InboxError> {
        InboxDesk::new(Whoami::new(sessions, memberships), inbox.clone(), inbox)
            .notification_preferences(token)
            .await
    }

    #[transactional(sessions, memberships, inbox, unavailable = InboxError::StoreUnreachable)]
    pub async fn set_notification_preferences(
        &self,
        token: &SessionToken,
        preferences: &NotificationPreferences,
    ) -> Result<NotificationPreferences, InboxError> {
        InboxDesk::new(Whoami::new(sessions, memberships), inbox.clone(), inbox)
            .set_notification_preferences(token, preferences)
            .await
    }

    /// Raises one notification for one account in one organisation, unless the
    /// account turned the in-app channel off for this kind. Not reachable over
    /// HTTP: future producers (see docs/chantiers/account-preferences.md) call
    /// this directly once they exist. No caller does yet.
    #[transactional(sessions, memberships, inbox, unavailable = InboxError::StoreUnreachable)]
    pub async fn notify(
        &self,
        account: AccountId,
        org: TenantId,
        kind: InboxItemKind,
        title: &str,
        body: &str,
        link: Option<&str>,
    ) -> Result<Option<InboxItem>, InboxError> {
        InboxDesk::new(Whoami::new(sessions, memberships), inbox.clone(), inbox)
            .notify(
                &InboxItemDraft {
                    account,
                    org,
                    kind,
                    title: title.into(),
                    body: body.into(),
                    link: link.map(Into::into),
                },
                self.clock.now().get() * 1_000,
            )
            .await
    }
}
