use core::future::Future;

use super::errors::PhotoError;
use super::value_objects::{AccountName, Photo, Pictured, Portrait, Preferences};
use crate::identity::AccountId;
use crate::store::StoreError;

pub trait Accounts {
    fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn portrait_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<Portrait>, StoreError>> + Send;

    fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn preferences_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<Preferences>, StoreError>> + Send;

    fn set_preferences(
        &self,
        account: AccountId,
        preferences: &Preferences,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl<T: Accounts + Sync> Accounts for &T {
    fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).rename(account, name)
    }

    fn portrait_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<Portrait>, StoreError>> + Send {
        (*self).portrait_of(account)
    }

    fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).set_portrait(account, portrait)
    }

    fn preferences_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<Preferences>, StoreError>> + Send {
        (*self).preferences_of(account)
    }

    fn set_preferences(
        &self,
        account: AccountId,
        preferences: &Preferences,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        (*self).set_preferences(account, preferences)
    }
}

impl<T: Accounts + Send + Sync> Accounts for std::sync::Arc<T> {
    fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().rename(account, name)
    }

    fn portrait_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<Portrait>, StoreError>> + Send {
        self.as_ref().portrait_of(account)
    }

    fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().set_portrait(account, portrait)
    }

    fn preferences_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<Preferences>, StoreError>> + Send {
        self.as_ref().preferences_of(account)
    }

    fn set_preferences(
        &self,
        account: AccountId,
        preferences: &Preferences,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.as_ref().set_preferences(account, preferences)
    }
}

pub trait Photos {
    fn put(
        &self,
        of: Pictured,
        photo: &Photo,
    ) -> impl Future<Output = Result<Portrait, PhotoError>> + Send;

    fn fetch(
        &self,
        of: Pictured,
        portrait: &Portrait,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, PhotoError>> + Send;

    fn remove(
        &self,
        of: Pictured,
        portrait: &Portrait,
    ) -> impl Future<Output = Result<(), PhotoError>> + Send;
}

impl<T: Photos + Sync> Photos for &T {
    fn put(
        &self,
        of: Pictured,
        photo: &Photo,
    ) -> impl Future<Output = Result<Portrait, PhotoError>> + Send {
        (*self).put(of, photo)
    }

    fn fetch(
        &self,
        of: Pictured,
        portrait: &Portrait,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, PhotoError>> + Send {
        (*self).fetch(of, portrait)
    }

    fn remove(
        &self,
        of: Pictured,
        portrait: &Portrait,
    ) -> impl Future<Output = Result<(), PhotoError>> + Send {
        (*self).remove(of, portrait)
    }
}

impl<T: Photos + Send + Sync> Photos for std::sync::Arc<T> {
    fn put(
        &self,
        of: Pictured,
        photo: &Photo,
    ) -> impl Future<Output = Result<Portrait, PhotoError>> + Send {
        self.as_ref().put(of, photo)
    }

    fn fetch(
        &self,
        of: Pictured,
        portrait: &Portrait,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, PhotoError>> + Send {
        self.as_ref().fetch(of, portrait)
    }

    fn remove(
        &self,
        of: Pictured,
        portrait: &Portrait,
    ) -> impl Future<Output = Result<(), PhotoError>> + Send {
        self.as_ref().remove(of, portrait)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoPhotos;

impl Photos for NoPhotos {
    async fn put(&self, _of: Pictured, _photo: &Photo) -> Result<Portrait, PhotoError> {
        Err(PhotoError::NotOffered)
    }

    async fn fetch(
        &self,
        _of: Pictured,
        _portrait: &Portrait,
    ) -> Result<Option<Vec<u8>>, PhotoError> {
        Err(PhotoError::NotOffered)
    }

    async fn remove(&self, _of: Pictured, _portrait: &Portrait) -> Result<(), PhotoError> {
        Err(PhotoError::NotOffered)
    }
}
