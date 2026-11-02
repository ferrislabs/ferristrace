use super::entities::Organisation;
use super::errors::SettingError;
use super::ports::{Memberships, Orgs};
use super::value_objects::{OrgName, OrgRole};
use super::whoami::Whoami;
use crate::identity::SessionStore;
use crate::profile::{Photo, PhotoKind, Photos, Pictured};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct OrgSettings<S, G, O, P> {
    whoami: Whoami<S, G>,
    orgs: O,
    photos: P,
}

impl<S, G, O, P> OrgSettings<S, G, O, P>
where
    S: SessionStore,
    G: Memberships,
    O: Orgs,
    P: Photos,
{
    pub fn new(whoami: Whoami<S, G>, orgs: O, photos: P) -> Self {
        Self {
            whoami,
            orgs,
            photos,
        }
    }

    pub async fn read(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Organisation, SettingError> {
        self.standing(token, org, OrgRole::Member).await?;
        self.card(org).await
    }

    pub async fn rename(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &OrgName,
    ) -> Result<Organisation, SettingError> {
        self.standing(token, org, OrgRole::Admin).await?;
        self.orgs.rename(org, name).await?;
        self.card(org).await
    }

    pub async fn put_logo(
        &self,
        token: &SessionToken,
        org: TenantId,
        logo: &Photo,
    ) -> Result<Organisation, SettingError> {
        self.standing(token, org, OrgRole::Admin).await?;
        let before = self.card(org).await?.logo;
        let kept = self.photos.put(Pictured::Org(org), logo).await?;
        self.orgs.set_logo(org, Some(&kept)).await?;
        if let Some(before) = before.filter(|before| before.token != kept.token) {
            let _ = self.photos.remove(Pictured::Org(org), &before).await;
        }
        self.card(org).await
    }

    pub async fn remove_logo(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Organisation, SettingError> {
        self.standing(token, org, OrgRole::Admin).await?;
        let Some(kept) = self.card(org).await?.logo else {
            return self.card(org).await;
        };
        self.orgs.set_logo(org, None).await?;
        let _ = self.photos.remove(Pictured::Org(org), &kept).await;
        self.card(org).await
    }

    pub async fn logo(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<(PhotoKind, Vec<u8>), SettingError> {
        self.standing(token, org, OrgRole::Member).await?;
        let kept = self.card(org).await?.logo.ok_or(SettingError::NoLogo)?;
        let bytes = self
            .photos
            .fetch(Pictured::Org(org), &kept)
            .await?
            .ok_or(SettingError::NoLogo)?;
        Ok((kept.kind, bytes))
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
        floor: OrgRole,
    ) -> Result<(), SettingError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(SettingError::NotYours),
            Some(role) if !role.at_least(floor) => Err(SettingError::NotYoursToChange),
            Some(_) => Ok(()),
        }
    }

    async fn card(&self, org: TenantId) -> Result<Organisation, SettingError> {
        self.orgs.only(org).await?.ok_or(SettingError::Gone)
    }
}
