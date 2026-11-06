use super::entities::{NewView, View, ViewChange};
use super::errors::ViewError;
use super::ports::{Creation, ExplorerViews, Saving};
use super::value_objects::{ViewId, ViewSignal, Visibility};
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct ViewDesk<S, G, V> {
    whoami: Whoami<S, G>,
    views: V,
}

impl<S, G, V> ViewDesk<S, G, V>
where
    S: SessionStore,
    G: Memberships,
    V: ExplorerViews,
{
    pub fn new(whoami: Whoami<S, G>, views: V) -> Self {
        Self { whoami, views }
    }

    pub async fn list(
        &self,
        token: &SessionToken,
        org: TenantId,
        signal: ViewSignal,
    ) -> Result<Vec<View>, ViewError> {
        let (me, _) = self.standing(token, org).await?;
        Ok(self.views.visible(org, signal, me).await?)
    }

    pub async fn create(
        &self,
        token: &SessionToken,
        org: TenantId,
        view: &NewView,
    ) -> Result<View, ViewError> {
        let (me, _) = self.standing(token, org).await?;
        match self.views.create(org, me, view).await? {
            Creation::Made(made) => Ok(made),
            Creation::NameTaken => Err(ViewError::NameTaken),
        }
    }

    pub async fn change(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ViewId,
        change: &ViewChange,
    ) -> Result<View, ViewError> {
        let (me, role) = self.standing(token, org).await?;
        let held = self.mine_to_change(org, id, me, role).await?;
        let shares = change
            .visibility
            .is_some_and(|wanted| wanted != held.visibility);
        if shares && held.created_by != Some(me) {
            return Err(ViewError::NotYoursToShare);
        }
        match self.views.change(org, id, change).await? {
            Saving::Missing => return Err(ViewError::NoSuchView),
            Saving::NameTaken => return Err(ViewError::NameTaken),
            Saving::Saved => {}
        }
        self.views.find(org, id).await?.ok_or(ViewError::NoSuchView)
    }

    pub async fn delete(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ViewId,
    ) -> Result<(), ViewError> {
        let (me, role) = self.standing(token, org).await?;
        self.mine_to_change(org, id, me, role).await?;
        if self.views.delete(org, id).await? {
            Ok(())
        } else {
            Err(ViewError::NoSuchView)
        }
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<(AccountId, OrgRole), ViewError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(ViewError::NotYours),
            Some(role) => Ok((who.account, role)),
        }
    }

    async fn mine_to_change(
        &self,
        org: TenantId,
        id: ViewId,
        me: AccountId,
        role: OrgRole,
    ) -> Result<View, ViewError> {
        let held = self
            .views
            .find(org, id)
            .await?
            .ok_or(ViewError::NoSuchView)?;
        let created_by_me = held.created_by == Some(me);
        if held.visibility == Visibility::Private && !created_by_me {
            return Err(ViewError::NoSuchView);
        }
        if created_by_me || role.at_least(OrgRole::Admin) {
            Ok(held)
        } else {
            Err(ViewError::NotYoursToChange)
        }
    }
}
