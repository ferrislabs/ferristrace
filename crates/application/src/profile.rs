use ferristrace_domain::{
    AccountId, AccountName, Photo, PhotoKind, Preferences, Profile, ProfileError, SessionToken,
    Who, Whoami,
};

use crate::registry::ProfileRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: ProfileRepos,
{
    #[transactional(sessions, memberships, accounts, unavailable = ProfileError::StoreUnreachable)]
    pub async fn profile(&self, token: &SessionToken) -> Result<Who, ProfileError> {
        Profile::new(Whoami::new(sessions, memberships), accounts, &self.photos)
            .read(token)
            .await
    }

    #[transactional(sessions, memberships, accounts, unavailable = ProfileError::StoreUnreachable)]
    pub async fn rename_profile(
        &self,
        token: &SessionToken,
        name: Option<&AccountName>,
    ) -> Result<Who, ProfileError> {
        Profile::new(Whoami::new(sessions, memberships), accounts, &self.photos)
            .rename(token, name)
            .await
    }

    #[transactional(sessions, memberships, accounts, unavailable = ProfileError::StoreUnreachable)]
    pub async fn put_profile_photo(
        &self,
        token: &SessionToken,
        photo: &Photo,
    ) -> Result<Who, ProfileError> {
        Profile::new(Whoami::new(sessions, memberships), accounts, &self.photos)
            .put_photo(token, photo)
            .await
    }

    #[transactional(sessions, memberships, accounts, unavailable = ProfileError::StoreUnreachable)]
    pub async fn remove_profile_photo(&self, token: &SessionToken) -> Result<Who, ProfileError> {
        Profile::new(Whoami::new(sessions, memberships), accounts, &self.photos)
            .remove_photo(token)
            .await
    }

    #[transactional(sessions, memberships, accounts, unavailable = ProfileError::StoreUnreachable)]
    pub async fn preferences(&self, token: &SessionToken) -> Result<Preferences, ProfileError> {
        Profile::new(Whoami::new(sessions, memberships), accounts, &self.photos)
            .preferences(token)
            .await
    }

    #[transactional(sessions, memberships, accounts, unavailable = ProfileError::StoreUnreachable)]
    pub async fn set_preferences(
        &self,
        token: &SessionToken,
        preferences: &Preferences,
    ) -> Result<Preferences, ProfileError> {
        Profile::new(Whoami::new(sessions, memberships), accounts, &self.photos)
            .set_preferences(token, preferences)
            .await
    }

    #[transactional(sessions, memberships, accounts, unavailable = ProfileError::StoreUnreachable)]
    pub async fn profile_photo(
        &self,
        token: &SessionToken,
        of: AccountId,
    ) -> Result<(PhotoKind, Vec<u8>), ProfileError> {
        Profile::new(Whoami::new(sessions, memberships), accounts, &self.photos)
            .photo(token, of)
            .await
    }
}
