use super::entities::Organisation;
use super::errors::SettingError;
use super::ports::{Memberships, Orgs};
use super::value_objects::{OrgName, OrgRole, OrgTimezone};
use super::whoami::Whoami;
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target};
use crate::identity::{AccountId, SessionStore};
use crate::profile::{Photo, PhotoKind, Photos, Pictured};
use crate::secret::SessionToken;
use crate::telemetry::{RetentionDays, TenantId};

#[derive(Debug)]
pub struct OrgSettings<S, G, O, P, A> {
    whoami: Whoami<S, G>,
    orgs: O,
    photos: P,
    trail: A,
}

impl<S, G, O, P, A> OrgSettings<S, G, O, P, A>
where
    S: SessionStore,
    G: Memberships,
    O: Orgs,
    P: Photos,
    A: AuditTrail,
{
    pub fn new(whoami: Whoami<S, G>, orgs: O, photos: P, trail: A) -> Self {
        Self {
            whoami,
            orgs,
            photos,
            trail,
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
        let by = self.standing(token, org, OrgRole::Admin).await?;
        let before = self.card(org).await?.name;
        self.orgs.rename(org, name).await?;
        let card = self.card(org).await?;
        self.note(
            org,
            by,
            Action::OrgRenamed,
            Detail::default()
                .with("from", before)
                .with("to", name.as_str()),
        )
        .await?;
        Ok(card)
    }

    pub async fn disband(&self, token: &SessionToken, org: TenantId) -> Result<(), SettingError> {
        match self.standing(token, org, OrgRole::Owner).await {
            Ok(_) => {}
            Err(SettingError::NotYoursToChange) => return Err(SettingError::OwnersOnlyToDisband),
            Err(refused) => return Err(refused),
        }
        let logo = self.card(org).await?.logo;
        if !self.orgs.disband(org).await? {
            return Err(SettingError::Gone);
        }
        if let Some(kept) = logo {
            let _ = self.photos.remove(Pictured::Org(org), &kept).await;
        }
        Ok(())
    }

    pub async fn put_logo(
        &self,
        token: &SessionToken,
        org: TenantId,
        logo: &Photo,
    ) -> Result<Organisation, SettingError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        let before = self.card(org).await?.logo;
        let kept = self.photos.put(Pictured::Org(org), logo).await?;
        self.orgs.set_logo(org, Some(&kept)).await?;
        self.note(
            org,
            by,
            Action::OrgLogoSet,
            Detail::default().with("kind", kept.kind),
        )
        .await?;
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
        let by = self.standing(token, org, OrgRole::Admin).await?;
        let Some(kept) = self.card(org).await?.logo else {
            return self.card(org).await;
        };
        self.orgs.set_logo(org, None).await?;
        self.note(
            org,
            by,
            Action::OrgLogoRemoved,
            Detail::default().with("kind", kept.kind),
        )
        .await?;
        let _ = self.photos.remove(Pictured::Org(org), &kept).await;
        self.card(org).await
    }

    pub async fn set_retention(
        &self,
        token: &SessionToken,
        org: TenantId,
        retention: Option<RetentionDays>,
    ) -> Result<Organisation, SettingError> {
        let by = match self.standing(token, org, OrgRole::Owner).await {
            Err(SettingError::NotYoursToChange) => return Err(SettingError::OwnersOnly),
            other => other?,
        };
        let before = self.card(org).await?;
        if let (Some(asked), Some(cap)) = (retention, before.retention_cap)
            && asked > cap
        {
            return Err(SettingError::AboveCap { cap });
        }
        self.orgs.set_retention(org, retention).await?;
        let card = self.card(org).await?;
        self.note(
            org,
            by,
            Action::OrgRetentionSet,
            Detail::default()
                .with("from", shown(before.retention))
                .with("to", shown(retention)),
        )
        .await?;
        Ok(card)
    }

    pub async fn set_timezone(
        &self,
        token: &SessionToken,
        org: TenantId,
        timezone: &OrgTimezone,
    ) -> Result<Organisation, SettingError> {
        let by = self.standing(token, org, OrgRole::Admin).await?;
        let before = self.card(org).await?.timezone;
        if &before == timezone {
            return self.card(org).await;
        }
        self.orgs.set_timezone(org, timezone).await?;
        let card = self.card(org).await?;
        self.note(
            org,
            by,
            Action::OrgTimezoneSet,
            Detail::default().with("from", before).with("to", timezone),
        )
        .await?;
        Ok(card)
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
    ) -> Result<AccountId, SettingError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(SettingError::NotYours),
            Some(role) if !role.at_least(floor) => Err(SettingError::NotYoursToChange),
            Some(_) => Ok(who.account),
        }
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        detail: Detail,
    ) -> Result<(), SettingError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(by),
                action,
                target: Target::org(org),
                detail,
            })
            .await?)
    }

    async fn card(&self, org: TenantId) -> Result<Organisation, SettingError> {
        self.orgs.only(org).await?.ok_or(SettingError::Gone)
    }
}

fn shown(retention: Option<RetentionDays>) -> String {
    retention.map_or_else(|| "default".to_owned(), |days| days.to_string())
}
