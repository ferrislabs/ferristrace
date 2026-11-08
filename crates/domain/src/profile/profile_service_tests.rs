use std::sync::Mutex;
use uuid::Uuid;

use super::errors::{BadAccountName, PhotoError, ProfileError};
use super::ports::{Accounts, NoPhotos, Photos};
use super::profile_service::Profile;
use super::value_objects::{
    AccountName, AccountTimezone, MAX_NAME_BYTES, Photo, PhotoKind, Pictured, Portrait,
    Preferences, RowDensity, StartPage, TimestampPrecision,
};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::TenantId;

const TOKEN: &str = "ft_ss_live";
const ACCOUNT: u128 = 9;
const SOMEBODY_ELSE: u128 = 10;

fn an_account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn a_name(spelling: &str) -> AccountName {
    AccountName::try_from(spelling).expect("an admitted name")
}

fn token() -> SessionToken {
    SessionToken::for_subject(TOKEN)
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
    prefs: Mutex<Option<Preferences>>,
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
    ) -> Result<(), StoreError> {
        assert_eq!(account, an_account(ACCOUNT));
        let chosen = name.map(|name| name.as_str().to_owned());
        self.writes.lock().expect("the lock").push(chosen.clone());
        *self.name.lock().expect("the lock") = chosen;
        Ok(())
    }

    async fn portrait_of(&self, account: AccountId) -> Result<Option<Portrait>, StoreError> {
        Ok(self.column_of(account).lock().expect("the lock").clone())
    }

    async fn set_portrait(
        &self,
        account: AccountId,
        portrait: Option<&Portrait>,
    ) -> Result<(), StoreError> {
        *self.column_of(account).lock().expect("the lock") = portrait.cloned();
        Ok(())
    }

    async fn preferences_of(&self, account: AccountId) -> Result<Option<Preferences>, StoreError> {
        assert_eq!(account, an_account(ACCOUNT));
        Ok(self.prefs.lock().expect("the lock").clone())
    }

    async fn set_preferences(
        &self,
        account: AccountId,
        preferences: &Preferences,
    ) -> Result<(), StoreError> {
        assert_eq!(account, an_account(ACCOUNT));
        *self.prefs.lock().expect("the lock") = Some(preferences.clone());
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
    async fn resolve(&self, asked: &SessionToken) -> Result<Session, SessionError> {
        if asked.identity().subject.as_str() != TOKEN {
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
async fn an_account_with_no_preferences_set_reads_the_defaults() {
    let (kept, shelf) = (Kept::default(), Shelf::default());

    let preferences = profile(&kept, &shelf)
        .preferences(&token())
        .await
        .expect("a read");

    assert_eq!(preferences, Preferences::default());
    assert_eq!(preferences.timezone.as_str(), "UTC");
    assert_eq!(preferences.precision, TimestampPrecision::Millis);
    assert_eq!(preferences.density, RowDensity::Compact);
    assert_eq!(preferences.start_page, StartPage::Overview);
}

#[tokio::test]
async fn chosen_preferences_are_written_and_read_back() {
    let (kept, shelf) = (Kept::default(), Shelf::default());
    let chosen = Preferences {
        timezone: AccountTimezone::try_from("Europe/Paris").expect("a timezone"),
        precision: TimestampPrecision::Nanos,
        density: RowDensity::Comfortable,
        start_page: StartPage::Traces,
    };

    let kept_back = profile(&kept, &shelf)
        .set_preferences(&token(), &chosen)
        .await
        .expect("a write");
    let read_back = profile(&kept, &shelf)
        .preferences(&token())
        .await
        .expect("a read");

    assert_eq!(kept_back, chosen);
    assert_eq!(read_back, chosen);
}

#[tokio::test]
async fn a_token_no_session_carries_writes_nothing() {
    let (kept, shelf) = (Kept::named("Inès Martin"), Shelf::default());

    let refused = profile(&kept, &shelf)
        .rename(
            &SessionToken::for_subject("ft_ss_nope"),
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
            &SessionToken::for_subject("ft_ss_nope"),
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

    use super::super::value_objects::MAX_NAME_BYTES;
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
