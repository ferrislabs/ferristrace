use ferristrace_domain::{
    AccountId, Email, InboxItemKind, Invitation, Member, OrgRole, Roster, RosterError,
    SessionToken, TenantId, Whoami,
};

use crate::registry::{InboxRepos, RosterRepos};
use crate::{Ferristrace, Stack, transactional};

const MEMBERS_LINK: &str = "/settings/members";

impl<S: Stack> Ferristrace<S>
where
    S::Repos: RosterRepos + InboxRepos,
{
    /// Best effort: a failed notification never undoes the member change
    /// that just happened, and the postgres adapter already logs the cause.
    async fn tell_admins(&self, org: TenantId, admins: Vec<AccountId>, title: &str, body: &str) {
        for account in admins {
            let _ = self
                .notify(
                    account,
                    org,
                    InboxItemKind::Membership,
                    title,
                    body,
                    Some(MEMBERS_LINK),
                )
                .await;
        }
    }

    #[transactional(sessions, memberships, roster, audit, unavailable = RosterError::StoreUnreachable)]
    pub async fn list_members(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<Member>, RosterError> {
        Roster::new(Whoami::new(sessions, memberships), roster, audit)
            .members(token, org)
            .await
    }

    #[transactional(sessions, memberships, roster, audit, unavailable = RosterError::StoreUnreachable)]
    pub async fn grant_member_role(
        &self,
        token: &SessionToken,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> Result<Member, RosterError> {
        Roster::new(Whoami::new(sessions, memberships), roster, audit)
            .grant(token, org, account, role)
            .await
    }

    #[transactional(sessions, memberships, roster, audit, unavailable = RosterError::StoreUnreachable)]
    pub async fn remove_member(
        &self,
        token: &SessionToken,
        org: TenantId,
        account: AccountId,
    ) -> Result<(), RosterError> {
        let notify = Roster::new(Whoami::new(sessions, memberships), roster, audit)
            .remove(token, org, account)
            .await?;
        self.tell_admins(
            org,
            notify,
            "A member left",
            "A member was removed from your organisation.",
        )
        .await;
        Ok(())
    }

    #[transactional(sessions, memberships, roster, audit, unavailable = RosterError::StoreUnreachable)]
    pub async fn invite_member(
        &self,
        token: &SessionToken,
        org: TenantId,
        email: &Email,
        role: OrgRole,
    ) -> Result<Invitation, RosterError> {
        let (invitation, notify) = Roster::new(Whoami::new(sessions, memberships), roster, audit)
            .invite(token, org, email, role)
            .await?;
        self.tell_admins(
            org,
            notify,
            "A member joined",
            &format!("{} joined as {}", email.as_str(), invitation.member.role),
        )
        .await;
        Ok(invitation)
    }
}
