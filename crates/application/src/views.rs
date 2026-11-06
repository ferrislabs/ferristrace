use ferristrace_domain::{
    NewView, SessionToken, TenantId, View, ViewChange, ViewDesk, ViewError, ViewId, ViewSignal,
    Whoami,
};

use crate::registry::ViewRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: ViewRepos,
{
    #[transactional(sessions, memberships, views, unavailable = ViewError::StoreUnreachable)]
    pub async fn list_views(
        &self,
        token: &SessionToken,
        org: TenantId,
        signal: ViewSignal,
    ) -> Result<Vec<View>, ViewError> {
        ViewDesk::new(Whoami::new(sessions, memberships), views)
            .list(token, org, signal)
            .await
    }

    #[transactional(sessions, memberships, views, unavailable = ViewError::StoreUnreachable)]
    pub async fn create_view(
        &self,
        token: &SessionToken,
        org: TenantId,
        view: &NewView,
    ) -> Result<View, ViewError> {
        ViewDesk::new(Whoami::new(sessions, memberships), views)
            .create(token, org, view)
            .await
    }

    #[transactional(sessions, memberships, views, unavailable = ViewError::StoreUnreachable)]
    pub async fn change_view(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ViewId,
        change: &ViewChange,
    ) -> Result<View, ViewError> {
        ViewDesk::new(Whoami::new(sessions, memberships), views)
            .change(token, org, id, change)
            .await
    }

    #[transactional(sessions, memberships, views, unavailable = ViewError::StoreUnreachable)]
    pub async fn delete_view(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ViewId,
    ) -> Result<(), ViewError> {
        ViewDesk::new(Whoami::new(sessions, memberships), views)
            .delete(token, org, id)
            .await
    }
}
