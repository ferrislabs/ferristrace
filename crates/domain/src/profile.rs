use core::future::Future;

use crate::explore::ReadError;
use crate::identity::AccountId;
use crate::membership::{Memberships, Who, Whoami, WhoamiError};
use crate::photo::{Photo, PhotoError, PhotoKind, Photos, Pictured, Portrait};
use crate::secret::SessionToken;
use crate::session::SessionStore;

const MAX_NAME_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadAccountName {
    #[error("a name cannot be only whitespace")]
    Blank,
    #[error("a name is at most {0} bytes")]
    TooLong(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountName(Box<str>);

impl AccountName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for AccountName {
    type Error = BadAccountName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim();
        if kept.len() > MAX_NAME_BYTES {
            return Err(BadAccountName::TooLong(MAX_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadAccountName::Blank);
        }
        Ok(Self(kept.into()))
    }
}

pub trait Accounts {
    fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;

    fn portrait_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<Portrait>, ReadError>> + Send;

    fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;
}

impl<T: Accounts + Sync> Accounts for &T {
    fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).rename(account, name)
    }

    fn portrait_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<Portrait>, ReadError>> + Send {
        (*self).portrait_of(account)
    }

    fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).set_portrait(account, portrait)
    }
}

impl<T: Accounts + Send + Sync> Accounts for std::sync::Arc<T> {
    fn rename(
        &self,
        account: AccountId,
        name: Option<&AccountName>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        self.as_ref().rename(account, name)
    }

    fn portrait_of(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<Portrait>, ReadError>> + Send {
        self.as_ref().portrait_of(account)
    }

    fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        self.as_ref().set_portrait(account, portrait)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("that account keeps no photo")]
    NoPhoto,
    #[error(transparent)]
    Photos(PhotoError),
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for ProfileError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadError> for ProfileError {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<PhotoError> for ProfileError {
    fn from(error: PhotoError) -> Self {
        match error {
            PhotoError::NotOffered => Self::Photos(PhotoError::NotOffered),
            PhotoError::Unreachable => Self::StoreUnreachable,
        }
    }
}

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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use uuid::Uuid;

    use super::{AccountName, Accounts, BadAccountName, MAX_NAME_BYTES, Profile, ProfileError};
    use crate::clock::UnixSeconds;
    use crate::credential::Email;
    use crate::explore::ReadError;
    use crate::identity::AccountId;
    use crate::membership::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
    use crate::photo::{NoPhotos, Photo, PhotoError, PhotoKind, Photos, Pictured, Portrait};
    use crate::secret::SessionToken;
    use crate::session::{Agent, Session, SessionError, SessionStore};
    use crate::tenant::TenantId;

    const TOKEN: &str = "ct_ss_live";
    const ACCOUNT: u128 = 9;
    const SOMEBODY_ELSE: u128 = 10;

    fn an_account(seed: u128) -> AccountId {
        AccountId::new(Uuid::from_u128(seed))
    }

    fn a_name(spelling: &str) -> AccountName {
        AccountName::try_from(spelling).expect("an admitted name")
    }

    fn token() -> SessionToken {
        SessionToken::from_plaintext(TOKEN)
    }

    fn a_png() -> Photo {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(b"a face");
        Photo::examined(bytes).expect("a png")
    }

    #[derive(Debug, Default)]
    struct Kept {
        name: Mutex<Option<String>>,
        mine: Mutex<Option<Portrait>>,
        theirs: Mutex<Option<Portrait>>,
        writes: Mutex<Vec<Option<String>>>,
    }

    impl Kept {
        fn named(name: &str) -> Self {
            Self {
                name: Mutex::new(Some(name.to_owned())),
                ..Self::default()
            }
        }

        fn holding(portrait: Portrait) -> Self {
            Self {
                mine: Mutex::new(Some(portrait)),
                ..Self::default()
            }
        }

        fn writes(&self) -> Vec<Option<String>> {
            self.writes.lock().expect("the lock").clone()
        }

        fn portrait(&self) -> Option<Portrait> {
            self.mine.lock().expect("the lock").clone()
        }

        fn column_of(&self, account: AccountId) -> &Mutex<Option<Portrait>> {
            if account == an_account(ACCOUNT) {
                &self.mine
            } else {
                &self.theirs
            }
        }
    }

    impl Accounts for &Kept {
        async fn rename(
            &self,
            account: AccountId,
            name: Option<&AccountName>,
        ) -> Result<(), ReadError> {
            assert_eq!(account, an_account(ACCOUNT));
            let chosen = name.map(|name| name.as_str().to_owned());
            self.writes.lock().expect("the lock").push(chosen.clone());
            *self.name.lock().expect("the lock") = chosen;
            Ok(())
        }

        async fn portrait_of(&self, account: AccountId) -> Result<Option<Portrait>, ReadError> {
            Ok(self.column_of(account).lock().expect("the lock").clone())
        }

        async fn set_portrait(
            &self,
            account: AccountId,
            portrait: Option<&Portrait>,
        ) -> Result<(), ReadError> {
            *self.column_of(account).lock().expect("the lock") = portrait.cloned();
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct Shelf {
        objects: Mutex<Vec<(String, Vec<u8>)>>,
        forgotten: Mutex<Vec<String>>,
        minted: Mutex<u8>,
    }

    impl Shelf {
        fn holding(token: &str, bytes: &[u8]) -> Self {
            Self {
                objects: Mutex::new(vec![(token.to_owned(), bytes.to_vec())]),
                ..Self::default()
            }
        }

        fn tokens(&self) -> Vec<String> {
            self.objects
                .lock()
                .expect("the lock")
                .iter()
                .map(|(token, _)| token.clone())
                .collect()
        }

        fn forgotten(&self) -> Vec<String> {
            self.forgotten.lock().expect("the lock").clone()
        }
    }

    impl Photos for &Shelf {
        async fn put(&self, _of: Pictured, photo: &Photo) -> Result<Portrait, PhotoError> {
            let mut minted = self.minted.lock().expect("the lock");
            *minted += 1;
            let token = format!("token{minted}");
            self.objects
                .lock()
                .expect("the lock")
                .push((token.clone(), photo.bytes().to_vec()));
            Ok(Portrait {
                token: token.into(),
                kind: photo.kind(),
            })
        }

        async fn fetch(
            &self,
            _of: Pictured,
            portrait: &Portrait,
        ) -> Result<Option<Vec<u8>>, PhotoError> {
            Ok(self
                .objects
                .lock()
                .expect("the lock")
                .iter()
                .find(|(token, _)| token.as_str() == &*portrait.token)
                .map(|(_, bytes)| bytes.clone()))
        }

        async fn remove(&self, _of: Pictured, portrait: &Portrait) -> Result<(), PhotoError> {
            self.forgotten
                .lock()
                .expect("the lock")
                .push(portrait.token.to_string());
            self.objects
                .lock()
                .expect("the lock")
                .retain(|(token, _)| token.as_str() != &*portrait.token);
            Ok(())
        }
    }

    struct SessionOf<'a>(&'a Kept);

    impl SessionStore for SessionOf<'_> {
        async fn issue(
            &self,
            _account: AccountId,
            _org: Option<TenantId>,
            _from: Option<&Agent>,
        ) -> Result<SessionToken, SessionError> {
            Ok(token())
        }

        async fn resolve(&self, asked: &SessionToken) -> Result<Session, SessionError> {
            if asked.exposed() != TOKEN {
                return Err(SessionError::Unknown);
            }
            Ok(Session {
                account: an_account(ACCOUNT),
                email: Email::try_from("person@example.com").expect("an address"),
                name: self
                    .0
                    .name
                    .lock()
                    .expect("the lock")
                    .clone()
                    .map(Into::into),
                photo: self.0.portrait(),
                org: None,
                expires_at: UnixSeconds::new(0),
            })
        }

        async fn revoke(&self, _asked: &SessionToken) -> Result<(), SessionError> {
            Ok(())
        }
    }

    struct OneOrg;

    impl Memberships for OneOrg {
        async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
            Ok(vec![Membership {
                org: TenantId::new(Uuid::from_u128(1)),
                name: "Acme".into(),
                role: OrgRole::Member,
            }])
        }
    }

    fn profile<'a>(
        kept: &'a Kept,
        shelf: &'a Shelf,
    ) -> Profile<SessionOf<'a>, OneOrg, &'a Kept, &'a Shelf> {
        Profile::new(Whoami::new(SessionOf(kept), OneOrg), kept, shelf)
    }

    #[tokio::test]
    async fn an_account_with_no_name_stands_on_its_address() {
        let (kept, shelf) = (Kept::default(), Shelf::default());

        let who = profile(&kept, &shelf)
            .read(&token())
            .await
            .expect("a standing");

        assert_eq!(who.name, None);
        assert_eq!(who.photo, None);
        assert_eq!(who.email.as_str(), "person@example.com");
    }

    #[tokio::test]
    async fn a_chosen_name_is_written_and_the_standing_carries_it_back() {
        let (kept, shelf) = (Kept::default(), Shelf::default());

        let who = profile(&kept, &shelf)
            .rename(&token(), Some(&a_name("Inès Martin")))
            .await
            .expect("a rename");

        assert_eq!(who.name.as_deref(), Some("Inès Martin"));
        assert_eq!(kept.writes(), vec![Some("Inès Martin".to_owned())]);
    }

    #[tokio::test]
    async fn emptying_the_name_takes_it_off_the_account() {
        let (kept, shelf) = (Kept::named("Inès Martin"), Shelf::default());

        let who = profile(&kept, &shelf)
            .rename(&token(), None)
            .await
            .expect("a rename");

        assert_eq!(who.name, None);
        assert_eq!(kept.writes(), vec![None]);
    }

    #[tokio::test]
    async fn a_token_no_session_carries_writes_nothing() {
        let (kept, shelf) = (Kept::named("Inès Martin"), Shelf::default());

        let refused = profile(&kept, &shelf)
            .rename(
                &SessionToken::from_plaintext("ct_ss_nope"),
                Some(&a_name("Somebody Else")),
            )
            .await;

        assert_eq!(refused, Err(ProfileError::NoSession));
        assert!(kept.writes().is_empty());
    }

    #[tokio::test]
    async fn a_photo_is_kept_and_the_standing_points_at_it() {
        let (kept, shelf) = (Kept::default(), Shelf::default());

        let who = profile(&kept, &shelf)
            .put_photo(&token(), &a_png())
            .await
            .expect("a photo");

        let portrait = who.photo.expect("the standing carries it");
        assert_eq!(portrait.kind, PhotoKind::Png);
        assert_eq!(shelf.tokens(), vec![portrait.token.to_string()]);
        assert_eq!(kept.portrait(), Some(portrait));
    }

    #[tokio::test]
    async fn a_store_that_refuses_leaves_the_account_as_it_was() {
        let kept = Kept::default();

        let refused = Profile::new(Whoami::new(SessionOf(&kept), OneOrg), &kept, NoPhotos)
            .put_photo(&token(), &a_png())
            .await;

        assert_eq!(
            refused,
            Err(ProfileError::Photos(PhotoError::NotOffered)),
            "a deployment with no store says so rather than failing vaguely"
        );
        assert_eq!(kept.portrait(), None);
    }

    #[tokio::test]
    async fn a_replaced_photo_is_forgotten_by_the_store() {
        let old = Portrait {
            token: "token0".into(),
            kind: PhotoKind::Jpeg,
        };
        let kept = Kept::holding(old.clone());
        let shelf = Shelf::holding("token0", b"an older face");

        let who = profile(&kept, &shelf)
            .put_photo(&token(), &a_png())
            .await
            .expect("a photo");

        let portrait = who.photo.expect("the standing carries it");
        assert_ne!(portrait.token, old.token, "a new photo is a new url");
        assert_eq!(shelf.forgotten(), vec!["token0".to_owned()]);
        assert_eq!(shelf.tokens(), vec![portrait.token.to_string()]);
    }

    #[tokio::test]
    async fn a_removed_photo_leaves_neither_a_column_nor_an_object() {
        let kept = Kept::holding(Portrait {
            token: "token0".into(),
            kind: PhotoKind::Png,
        });
        let shelf = Shelf::holding("token0", b"a face");

        let who = profile(&kept, &shelf)
            .remove_photo(&token())
            .await
            .expect("a removal");

        assert_eq!(who.photo, None);
        assert_eq!(kept.portrait(), None);
        assert!(shelf.tokens().is_empty());
        assert_eq!(shelf.forgotten(), vec!["token0".to_owned()]);
    }

    #[tokio::test]
    async fn removing_a_photo_nobody_kept_is_not_an_error() {
        let (kept, shelf) = (Kept::default(), Shelf::default());

        let who = profile(&kept, &shelf)
            .remove_photo(&token())
            .await
            .expect("a removal");

        assert_eq!(who.photo, None);
        assert!(shelf.forgotten().is_empty(), "there was nothing to forget");
    }

    #[tokio::test]
    async fn a_photo_is_read_back_with_what_it_is() {
        let (kept, shelf) = (Kept::default(), Shelf::default());
        let photo = a_png();
        profile(&kept, &shelf)
            .put_photo(&token(), &photo)
            .await
            .expect("a photo");

        let (kind, bytes) = profile(&kept, &shelf)
            .photo(&token(), an_account(ACCOUNT))
            .await
            .expect("it is served");

        assert_eq!(kind, PhotoKind::Png);
        assert_eq!(bytes, photo.bytes());
    }

    #[tokio::test]
    async fn an_account_that_keeps_no_photo_says_so_rather_than_answering_nothing() {
        let (kept, shelf) = (Kept::default(), Shelf::default());

        let refused = profile(&kept, &shelf)
            .photo(&token(), an_account(SOMEBODY_ELSE))
            .await;

        assert_eq!(refused, Err(ProfileError::NoPhoto));
    }

    #[tokio::test]
    async fn a_photo_is_not_served_to_a_token_no_session_carries() {
        let (kept, shelf) = (Kept::default(), Shelf::default());
        profile(&kept, &shelf)
            .put_photo(&token(), &a_png())
            .await
            .expect("a photo");

        let refused = profile(&kept, &shelf)
            .photo(
                &SessionToken::from_plaintext("ct_ss_nope"),
                an_account(ACCOUNT),
            )
            .await;

        assert_eq!(refused, Err(ProfileError::NoSession));
    }

    #[test]
    fn a_name_is_kept_without_the_space_around_it() {
        assert_eq!(a_name("  Inès Martin \n").as_str(), "Inès Martin");
    }

    #[test]
    fn a_name_that_is_only_space_is_no_name() {
        assert_eq!(AccountName::try_from(""), Err(BadAccountName::Blank));
        assert_eq!(AccountName::try_from("   \t\n"), Err(BadAccountName::Blank));
    }

    #[test]
    fn a_name_past_the_ceiling_is_refused() {
        let long = "a".repeat(MAX_NAME_BYTES + 1);

        assert_eq!(
            AccountName::try_from(long.as_str()),
            Err(BadAccountName::TooLong(MAX_NAME_BYTES))
        );
        assert!(AccountName::try_from("a".repeat(MAX_NAME_BYTES).as_str()).is_ok());
    }

    #[test]
    fn the_ceiling_counts_bytes_rather_than_characters() {
        let wide = "é".repeat(MAX_NAME_BYTES / 2 + 1);

        assert!(wide.chars().count() < MAX_NAME_BYTES);
        assert_eq!(
            AccountName::try_from(wide.as_str()),
            Err(BadAccountName::TooLong(MAX_NAME_BYTES))
        );
    }

    mod proptests {
        use proptest::prelude::*;

        use super::super::MAX_NAME_BYTES;
        use super::{AccountName, BadAccountName};

        fn a_printable_run(bytes: usize) -> impl Strategy<Value = String> {
            prop::collection::vec(0x21u8..=0x7eu8, 1..=bytes)
                .prop_map(|bytes| String::from_utf8(bytes).expect("ascii bytes"))
        }

        proptest! {
            #[test]
            fn a_name_the_constructor_admits_round_trips_once_padding_is_stripped(
                inner in a_printable_run(MAX_NAME_BYTES),
                leading in prop::collection::vec(prop_oneof![Just(' '), Just('\t')], 0..3),
                trailing in prop::collection::vec(prop_oneof![Just(' '), Just('\t')], 0..3),
            ) {
                let leading: String = leading.into_iter().collect();
                let trailing: String = trailing.into_iter().collect();
                let padded = format!("{leading}{inner}{trailing}");

                let name = AccountName::try_from(padded.as_str()).expect("a name within the ceiling");

                prop_assert_eq!(name.as_str(), inner.as_str());
            }

            #[test]
            fn a_name_one_byte_past_the_ceiling_is_refused_whatever_it_contains(
                over in prop::collection::vec(0x21u8..=0x7eu8, MAX_NAME_BYTES + 1)
                    .prop_map(|bytes| String::from_utf8(bytes).expect("ascii bytes"))
            ) {
                prop_assert_eq!(
                    AccountName::try_from(over.as_str()),
                    Err(BadAccountName::TooLong(MAX_NAME_BYTES))
                );
            }
        }
    }
}
