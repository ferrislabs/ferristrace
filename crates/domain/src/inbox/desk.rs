use super::entities::{InboxItem, InboxItemDraft};
use super::errors::InboxError;
use super::ports::{InboxStore, NotificationSettings};
use super::value_objects::{InboxItemId, NotificationPreferences, PAGE_LIMIT};
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct InboxDesk<S, G, I, N> {
    whoami: Whoami<S, G>,
    store: I,
    settings: N,
}

impl<S, G, I, N> InboxDesk<S, G, I, N>
where
    S: SessionStore,
    G: Memberships,
    I: InboxStore,
    N: NotificationSettings,
{
    pub fn new(whoami: Whoami<S, G>, store: I, settings: N) -> Self {
        Self {
            whoami,
            store,
            settings,
        }
    }

    async fn standing(&self, token: &SessionToken, org: TenantId) -> Result<AccountId, InboxError> {
        let who = self.whoami.of(token).await?;
        if who.role_in(org).is_none() {
            return Err(InboxError::NotYours);
        }
        Ok(who.account)
    }

    pub async fn list(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<InboxItem>, InboxError> {
        let account = self.standing(token, org).await?;
        Ok(self.store.list(account, org, PAGE_LIMIT).await?)
    }

    pub async fn mark_read(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: InboxItemId,
        now_ms: u64,
    ) -> Result<(), InboxError> {
        let account = self.standing(token, org).await?;
        if !self.store.mark_read(account, org, id, now_ms).await? {
            return Err(InboxError::NoSuchItem);
        }
        Ok(())
    }

    pub async fn mark_all_read(
        &self,
        token: &SessionToken,
        org: TenantId,
        now_ms: u64,
    ) -> Result<u64, InboxError> {
        let account = self.standing(token, org).await?;
        Ok(self.store.mark_all_read(account, org, now_ms).await?)
    }

    pub async fn notification_preferences(
        &self,
        token: &SessionToken,
    ) -> Result<NotificationPreferences, InboxError> {
        let who = self.whoami.of(token).await?;
        Ok(self.settings.of(who.account).await?.unwrap_or_default())
    }

    pub async fn set_notification_preferences(
        &self,
        token: &SessionToken,
        preferences: &NotificationPreferences,
    ) -> Result<NotificationPreferences, InboxError> {
        let who = self.whoami.of(token).await?;
        self.settings.set(who.account, preferences).await?;
        Ok(preferences.clone())
    }

    /// Called by a future producer (not over HTTP): raises one item for one
    /// account in one organisation, unless that account turned the in-app
    /// channel off for this kind - then there is nothing to raise and this
    /// returns `None`. No session of its own - the caller already knows who
    /// it is raising this for.
    pub async fn notify(
        &self,
        draft: &InboxItemDraft,
        now_ms: u64,
    ) -> Result<Option<InboxItem>, InboxError> {
        let preferences = self.settings.of(draft.account).await?.unwrap_or_default();
        if !preferences.channels_for(draft.kind).in_app {
            return Ok(None);
        }
        Ok(Some(self.store.create(draft, now_ms).await?))
    }
}
