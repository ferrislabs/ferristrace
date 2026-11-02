use ferristrace_domain::{
    AccountId, Email, Invitation, Member, OrgRole, Roster, RosterError, SessionToken, TenantId,
    Whoami,
};

use crate::registry::RosterRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: RosterRepos,
{
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
        Roster::new(Whoami::new(sessions, memberships), roster, audit)
            .remove(token, org, account)
            .await
    }

    #[transactional(sessions, memberships, roster, audit, unavailable = RosterError::StoreUnreachable)]
    pub async fn invite_member(
        &self,
        token: &SessionToken,
        org: TenantId,
        email: &Email,
        role: OrgRole,
    ) -> Result<Invitation, RosterError> {
        Roster::new(Whoami::new(sessions, memberships), roster, audit)
            .invite(token, org, email, role)
            .await
    }
}
