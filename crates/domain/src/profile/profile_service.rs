use super::errors::ProfileError;
use super::ports::{Accounts, Photos};
use super::value_objects::{AccountName, Photo, PhotoKind, Pictured};
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, Who, Whoami};
use crate::secret::SessionToken;

#[derive(Debug)]
pub struct Profile<S, G, A, P> {
    whoami: Whoami<S, G>,
    accounts: A,
    photos: P,
}

impl<S, G, A, P> Profile<S, G, A, P>
where
    S: SessionStore,
    G: Memberships,
    A: Accounts,
    P: Photos,
{
    pub fn new(whoami: Whoami<S, G>, accounts: A, photos: P) -> Self {
        Self {
            whoami,
            accounts,
            photos,
        }
    }

    pub async fn read(&self, token: &SessionToken) -> Result<Who, ProfileError> {
        Ok(self.whoami.of(token).await?)
    }

    pub async fn rename(
        &self,
        token: &SessionToken,
        name: Option<&AccountName>,
    ) -> Result<Who, ProfileError> {
        let who = self.read(token).await?;
        self.accounts.rename(who.account, name).await?;
        self.read(token).await
    }

    pub async fn put_photo(
        &self,
        token: &SessionToken,
        photo: &Photo,
    ) -> Result<Who, ProfileError> {
        let who = self.read(token).await?;
        let before = self.accounts.portrait_of(who.account).await?;
        let kept = self
            .photos
            .put(Pictured::Account(who.account), photo)
            .await?;
        self.accounts.set_portrait(who.account, Some(&kept)).await?;
        if let Some(before) = before.filter(|before| before.token != kept.token) {
            let _ = self
                .photos
                .remove(Pictured::Account(who.account), &before)
                .await;
        }
        self.read(token).await
    }

    pub async fn remove_photo(&self, token: &SessionToken) -> Result<Who, ProfileError> {
        let who = self.read(token).await?;
        let Some(kept) = self.accounts.portrait_of(who.account).await? else {
            return Ok(who);
        };
        self.accounts.set_portrait(who.account, None).await?;
        let _ = self
            .photos
            .remove(Pictured::Account(who.account), &kept)
            .await;
        self.read(token).await
    }

    pub async fn photo(
        &self,
        token: &SessionToken,
        of: AccountId,
    ) -> Result<(PhotoKind, Vec<u8>), ProfileError> {
        self.read(token).await?;
        let kept = self
            .accounts
            .portrait_of(of)
            .await?
            .ok_or(ProfileError::NoPhoto)?;
        let bytes = self
            .photos
            .fetch(Pictured::Account(of), &kept)
            .await?
            .ok_or(ProfileError::NoPhoto)?;
        Ok((kept.kind, bytes))
    }
}
