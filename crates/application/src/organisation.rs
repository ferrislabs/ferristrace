use ferristrace_domain::{
    OrgName, OrgSettings, Organisation, Photo, PhotoKind, SessionToken, SettingError, TenantId,
    Whoami,
};

use crate::registry::OrgRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: OrgRepos,
{
    #[transactional(sessions, memberships, orgs, unavailable = SettingError::StoreUnreachable)]
    pub async fn org_card(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Organisation, SettingError> {
        OrgSettings::new(Whoami::new(sessions, memberships), orgs, &self.photos)
            .read(token, org)
            .await
    }

    #[transactional(sessions, memberships, orgs, unavailable = SettingError::StoreUnreachable)]
    pub async fn rename_org(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &OrgName,
    ) -> Result<Organisation, SettingError> {
        OrgSettings::new(Whoami::new(sessions, memberships), orgs, &self.photos)
            .rename(token, org, name)
            .await
    }

    #[transactional(sessions, memberships, orgs, unavailable = SettingError::StoreUnreachable)]
    pub async fn put_org_logo(
        &self,
        token: &SessionToken,
        org: TenantId,
        logo: &Photo,
    ) -> Result<Organisation, SettingError> {
        OrgSettings::new(Whoami::new(sessions, memberships), orgs, &self.photos)
            .put_logo(token, org, logo)
            .await
    }

    #[transactional(sessions, memberships, orgs, unavailable = SettingError::StoreUnreachable)]
    pub async fn remove_org_logo(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Organisation, SettingError> {
        OrgSettings::new(Whoami::new(sessions, memberships), orgs, &self.photos)
            .remove_logo(token, org)
            .await
    }

    #[transactional(sessions, memberships, orgs, unavailable = SettingError::StoreUnreachable)]
    pub async fn org_logo(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<(PhotoKind, Vec<u8>), SettingError> {
        OrgSettings::new(Whoami::new(sessions, memberships), orgs, &self.photos)
            .logo(token, org)
            .await
    }
}
