use core::future::Future;

use ferristrace_domain::{
    ApiKey, Clock, Expired, Expiry, ExpiryNotice, ExpiryThreshold, InboxItemKind, IssuedKey,
    KeyError, KeyId, KeyName, KeyRing, KeyScopes, KeyTtl, ProjectId, Revocation, SessionToken,
    TenantId, Whoami,
};

use crate::registry::{AccessRepos, KeyExpiryRepos};
use crate::{Ferristrace, Stack, transactional};

const KEYS_LINK: &str = "/settings/keys";

impl<S: Stack> Ferristrace<S>
where
    S::Repos: AccessRepos,
{
    #[transactional(sessions, memberships, api_keys, projects, audit, unavailable = KeyError::StoreUnreachable)]
    pub async fn list_keys(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<ApiKey>, KeyError> {
        KeyRing::new(
            Whoami::new(sessions, memberships),
            api_keys,
            projects,
            audit,
        )
        .list(token, org)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, audit, unavailable = KeyError::StoreUnreachable)]
    pub async fn issue_key(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        ttl: KeyTtl,
    ) -> Result<IssuedKey, KeyError> {
        KeyRing::new(
            Whoami::new(sessions, memberships),
            api_keys,
            projects,
            audit,
        )
        .issue(token, org, project, name, scopes, ttl)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, audit, unavailable = KeyError::StoreUnreachable)]
    pub async fn revoke_key(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: KeyId,
    ) -> Result<(), KeyError> {
        KeyRing::new(
            Whoami::new(sessions, memberships),
            api_keys,
            projects,
            audit,
        )
        .revoke(token, org, id)
        .await
    }
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: KeyExpiryRepos,
{
    #[allow(clippy::manual_async_fn)]
    pub fn expire_keys(&self) -> impl Future<Output = Result<Expired, KeyError>> + Send {
        async move { self.expire_keys_once().await }
    }

    #[transactional(api_keys, roster, alert_duty, audit, unavailable = KeyError::StoreUnreachable)]
    async fn expire_keys_once(&self) -> Result<Expired, KeyError> {
        let expired = Expiry::new(api_keys, roster, alert_duty, audit)
            .tick(self.clock.now())
            .await?;
        if let Expired::Worked { revoked, noticed } = &expired {
            for revocation in revoked {
                self.tell_of_revocation(revocation).await;
            }
            for notice in noticed {
                self.tell_of_notice(notice).await;
            }
        }
        Ok(expired)
    }

    /// Best effort: a failed notification never undoes the revocation or
    /// forgets the notice flag already set, since the postgres adapter
    /// already logs the cause downstream.
    async fn tell_of_revocation(&self, revocation: &Revocation) {
        for account in &revocation.notify {
            let _ = self
                .notify(
                    *account,
                    revocation.key.org,
                    InboxItemKind::ApiKeyExpiring,
                    "A key was revoked",
                    &format!("{} expired and was revoked.", revocation.key.name),
                    Some(KEYS_LINK),
                )
                .await;
        }
    }

    async fn tell_of_notice(&self, notice: &ExpiryNotice) {
        let soon = match notice.threshold {
            ExpiryThreshold::Days7 => "in 7 days",
            ExpiryThreshold::Day1 => "in 1 day",
        };
        for account in &notice.notify {
            let _ = self
                .notify(
                    *account,
                    notice.key.org,
                    InboxItemKind::ApiKeyExpiring,
                    "A key is about to expire",
                    &format!("{} expires {soon}.", notice.key.name),
                    Some(KEYS_LINK),
                )
                .await;
        }
    }
}
