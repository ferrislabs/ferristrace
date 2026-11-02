use std::sync::Mutex;
use uuid::Uuid;

use super::entities::{Membership, Organisation};
use super::errors::{BadName, SettingError, WhoamiError};
use super::org_settings::OrgSettings;
use super::ports::{Memberships, Orgs};
use super::value_objects::{MAX_ORG_NAME_BYTES, OrgName, OrgRole};
use super::whoami::Whoami;
use crate::audit::{Action, Actor, Target};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Agent, Email, Session, SessionError, SessionStore};
use crate::profile::{NoPhotos, Photo, PhotoError, PhotoKind, Photos, Pictured, Portrait};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::TenantId;
use crate::testing::Notebook;

const TOKEN: &str = "ft_ss_live";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const FOUNDED: u64 = 1_700_000_000_000_000_000;

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn a_name(spelling: &str) -> OrgName {
    OrgName::try_from(spelling).expect("an admitted name")
}

struct OneSession;

impl SessionStore for OneSession {
    async fn issue(
        &self,
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Ok(SessionToken::from_plaintext(TOKEN))
    }

    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.exposed() == TOKEN {
            Ok(Session {
                account: AccountId::new(Uuid::from_u128(9)),
                email: Email::try_from("person@example.com").expect("an address"),
                name: None,
                photo: None,
                org: None,
                expires_at: UnixSeconds::new(0),
            })
        } else {
            Err(SessionError::Unknown)
        }
    }

    async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
        Ok(())
    }
}

struct MemberOfMine(OrgRole);

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: an_org(MINE),
            name: "Acme".into(),
            role: self.0,
        }])
    }
}

#[derive(Debug)]
struct Kept {
    name: Mutex<Option<String>>,
    renames: Mutex<Vec<String>>,
    logo: Mutex<Option<Portrait>>,
}

impl Kept {
    fn holding(name: &str) -> Self {
        Self {
            name: Mutex::new(Some(name.to_owned())),
            renames: Mutex::default(),
            logo: Mutex::default(),
        }
    }

    fn gone() -> Self {
        Self {
            name: Mutex::new(None),
            renames: Mutex::default(),
            logo: Mutex::default(),
        }
    }

    fn renames(&self) -> Vec<String> {
        self.renames.lock().expect("the lock").clone()
    }
}

impl Orgs for &Kept {
    async fn only(&self, org: TenantId) -> Result<Option<Organisation>, StoreError> {
        Ok(self
            .name
            .lock()
            .expect("the lock")
            .clone()
            .map(|name| Organisation {
                id: org,
                name: name.into(),
                plan: "Self-hosted".into(),
                created_at_unix_nanos: FOUNDED,
                logo: self.logo.lock().expect("the lock").clone(),
            }))
    }

    async fn rename(&self, _org: TenantId, name: &OrgName) -> Result<(), StoreError> {
        self.renames
            .lock()
            .expect("the lock")
            .push(name.as_str().to_owned());
        *self.name.lock().expect("the lock") = Some(name.as_str().to_owned());
        Ok(())
    }

    async fn set_logo(&self, _org: TenantId, logo: Option<&Portrait>) -> Result<(), StoreError> {
        *self.logo.lock().expect("the lock") = logo.cloned();
        Ok(())
    }
}

fn settings(
    role: OrgRole,
    kept: &Kept,
) -> OrgSettings<OneSession, MemberOfMine, &Kept, NoPhotos, Notebook> {
    noting(role, kept, &Notebook::default())
}

fn noting<'a>(
    role: OrgRole,
    kept: &'a Kept,
    notebook: &Notebook,
) -> OrgSettings<OneSession, MemberOfMine, &'a Kept, NoPhotos, Notebook> {
    OrgSettings::new(
        Whoami::new(OneSession, MemberOfMine(role)),
        kept,
        NoPhotos,
        notebook.clone(),
    )
}

#[derive(Debug, Default)]
struct Shelf {
    minted: Mutex<u32>,
    objects: Mutex<Vec<(String, Vec<u8>)>>,
    forgotten: Mutex<Vec<String>>,
    owners: Mutex<Vec<Pictured>>,
}

impl Shelf {
    fn forgotten(&self) -> Vec<String> {
        self.forgotten.lock().expect("the lock").clone()
    }

    fn owners(&self) -> Vec<Pictured> {
        self.owners.lock().expect("the lock").clone()
    }
}

impl Photos for &Shelf {
    async fn put(&self, of: Pictured, photo: &Photo) -> Result<Portrait, PhotoError> {
        self.owners.lock().expect("the lock").push(of);
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
        of: Pictured,
        portrait: &Portrait,
    ) -> Result<Option<Vec<u8>>, PhotoError> {
        self.owners.lock().expect("the lock").push(of);
        Ok(self
            .objects
            .lock()
            .expect("the lock")
            .iter()
            .find(|(token, _)| token.as_str() == &*portrait.token)
            .map(|(_, bytes)| bytes.clone()))
    }

    async fn remove(&self, of: Pictured, portrait: &Portrait) -> Result<(), PhotoError> {
        self.owners.lock().expect("the lock").push(of);
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

fn marked<'a>(
    role: OrgRole,
    kept: &'a Kept,
    shelf: &'a Shelf,
) -> OrgSettings<OneSession, MemberOfMine, &'a Kept, &'a Shelf, Notebook> {
    marked_noting(role, kept, shelf, &Notebook::default())
}

fn marked_noting<'a>(
    role: OrgRole,
    kept: &'a Kept,
    shelf: &'a Shelf,
    notebook: &Notebook,
) -> OrgSettings<OneSession, MemberOfMine, &'a Kept, &'a Shelf, Notebook> {
    OrgSettings::new(
        Whoami::new(OneSession, MemberOfMine(role)),
        kept,
        shelf,
        notebook.clone(),
    )
}

fn a_png() -> Photo {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend_from_slice(b"and then whatever an encoder wrote");
    Photo::examined(bytes).expect("a png")
}

fn token() -> SessionToken {
    SessionToken::from_plaintext(TOKEN)
}

#[tokio::test]
async fn a_member_reads_the_card_of_its_own_organisation() {
    let kept = Kept::holding("Acme");

    let card = settings(OrgRole::Member, &kept)
        .read(&token(), an_org(MINE))
        .await
        .expect("a member may read");

    assert_eq!(
        card,
        Organisation {
            id: an_org(MINE),
            name: "Acme".into(),
            plan: "Self-hosted".into(),
            created_at_unix_nanos: FOUNDED,
            logo: None,
        }
    );
}

#[tokio::test]
async fn the_card_of_another_organisation_is_refused() {
    let kept = Kept::holding("Acme");

    let refused = settings(OrgRole::Owner, &kept)
        .read(&token(), an_org(THEIRS))
        .await;

    assert_eq!(refused, Err(SettingError::NotYours));
}

#[tokio::test]
async fn a_token_no_session_carries_reads_nothing() {
    let kept = Kept::holding("Acme");

    let refused = settings(OrgRole::Owner, &kept)
        .read(&SessionToken::from_plaintext("ft_ss_nope"), an_org(MINE))
        .await;

    assert_eq!(refused, Err(SettingError::NoSession));
}

#[tokio::test]
async fn an_administrator_renames_it_and_reads_the_new_name_back() {
    let kept = Kept::holding("Acme");

    let card = settings(OrgRole::Admin, &kept)
        .rename(&token(), an_org(MINE), &a_name("Acme Payments"))
        .await
        .expect("an administrator may rename");

    assert_eq!(&*card.name, "Acme Payments");
    assert_eq!(kept.renames(), vec!["Acme Payments".to_owned()]);
}

#[tokio::test]
async fn a_member_cannot_rename_it_and_the_store_is_never_asked() {
    let kept = Kept::holding("Acme");

    let refused = settings(OrgRole::Member, &kept)
        .rename(&token(), an_org(MINE), &a_name("Mine Now"))
        .await;

    assert_eq!(refused, Err(SettingError::NotYoursToChange));
    assert!(kept.renames().is_empty());
}

#[tokio::test]
async fn renaming_an_organisation_this_session_is_no_part_of_says_no_such_thing() {
    let kept = Kept::holding("Acme");

    let refused = settings(OrgRole::Owner, &kept)
        .rename(&token(), an_org(THEIRS), &a_name("Theirs Now"))
        .await;

    assert_eq!(refused, Err(SettingError::NotYours));
    assert!(kept.renames().is_empty());
}

#[tokio::test]
async fn an_organisation_the_membership_outlived_is_gone_rather_than_blank() {
    let kept = Kept::gone();

    let refused = settings(OrgRole::Owner, &kept)
        .read(&token(), an_org(MINE))
        .await;

    assert_eq!(refused, Err(SettingError::Gone));
}

#[test]
fn a_name_is_kept_without_the_space_around_it() {
    assert_eq!(a_name("  Acme Payments \n").as_str(), "Acme Payments");
}

#[test]
fn a_name_that_is_only_space_is_as_blank_as_an_empty_one() {
    assert_eq!(OrgName::try_from(""), Err(BadName::Blank));
    assert_eq!(OrgName::try_from("   \t\n"), Err(BadName::Blank));
}

#[test]
fn a_name_past_the_ceiling_is_refused() {
    let long = "a".repeat(MAX_ORG_NAME_BYTES + 1);

    assert_eq!(
        OrgName::try_from(long.as_str()),
        Err(BadName::TooLong(MAX_ORG_NAME_BYTES))
    );
    assert!(OrgName::try_from("a".repeat(MAX_ORG_NAME_BYTES).as_str()).is_ok());
}

#[test]
fn the_ceiling_counts_bytes_rather_than_characters() {
    let wide = "\u{e9}".repeat(MAX_ORG_NAME_BYTES / 2 + 1);

    assert!(wide.chars().count() < MAX_ORG_NAME_BYTES);
    assert_eq!(
        OrgName::try_from(wide.as_str()),
        Err(BadName::TooLong(MAX_ORG_NAME_BYTES))
    );
}

#[tokio::test]
async fn an_administrator_keeps_a_logo_and_the_card_carries_it() {
    let kept = Kept::holding("Acme");
    let shelf = Shelf::default();

    let card = marked(OrgRole::Admin, &kept, &shelf)
        .put_logo(&token(), an_org(MINE), &a_png())
        .await
        .expect("an administrator may keep one");

    let logo = card.logo.expect("a logo");
    assert_eq!(&*logo.token, "token1");
    assert_eq!(logo.kind, PhotoKind::Png);
    assert_eq!(shelf.owners(), vec![Pictured::Org(an_org(MINE))]);
}

#[tokio::test]
async fn a_replacement_changes_the_token_and_forgets_the_one_before_it() {
    let kept = Kept::holding("Acme");
    let shelf = Shelf::default();

    marked(OrgRole::Admin, &kept, &shelf)
        .put_logo(&token(), an_org(MINE), &a_png())
        .await
        .expect("a first logo");
    let card = marked(OrgRole::Admin, &kept, &shelf)
        .put_logo(&token(), an_org(MINE), &a_png())
        .await
        .expect("a replacement");

    assert_eq!(&*card.logo.expect("a logo").token, "token2");
    assert_eq!(shelf.forgotten(), vec!["token1".to_owned()]);
}

#[tokio::test]
async fn a_plain_member_may_not_keep_a_logo() {
    let kept = Kept::holding("Acme");
    let shelf = Shelf::default();

    let refused = marked(OrgRole::Member, &kept, &shelf)
        .put_logo(&token(), an_org(MINE), &a_png())
        .await
        .expect_err("a member may not");

    assert_eq!(refused, SettingError::NotYoursToChange);
    assert_eq!(shelf.owners(), Vec::new(), "nothing reached the store");
}

#[tokio::test]
async fn removing_a_logo_twice_is_not_an_error() {
    let kept = Kept::holding("Acme");
    let shelf = Shelf::default();
    let put = marked(OrgRole::Admin, &kept, &shelf);
    put.put_logo(&token(), an_org(MINE), &a_png())
        .await
        .expect("a logo");

    let once = put
        .remove_logo(&token(), an_org(MINE))
        .await
        .expect("the first removal");
    let twice = put
        .remove_logo(&token(), an_org(MINE))
        .await
        .expect("the second is no error");

    assert!(once.logo.is_none());
    assert!(twice.logo.is_none());
    assert_eq!(shelf.forgotten(), vec!["token1".to_owned()]);
}

#[tokio::test]
async fn a_member_reads_the_logo_and_a_stranger_is_told_there_is_no_such_organisation() {
    let kept = Kept::holding("Acme");
    let shelf = Shelf::default();
    marked(OrgRole::Admin, &kept, &shelf)
        .put_logo(&token(), an_org(MINE), &a_png())
        .await
        .expect("a logo");

    let (kind, bytes) = marked(OrgRole::Member, &kept, &shelf)
        .logo(&token(), an_org(MINE))
        .await
        .expect("a member may read");
    let refused = marked(OrgRole::Owner, &kept, &shelf)
        .logo(&token(), an_org(THEIRS))
        .await
        .expect_err("a stranger may not");

    assert_eq!(kind, PhotoKind::Png);
    assert_eq!(bytes, a_png().bytes());
    assert_eq!(refused, SettingError::NotYours);
}

#[tokio::test]
async fn an_organisation_that_keeps_no_logo_is_not_an_error_on_its_card() {
    let kept = Kept::holding("Acme");

    let card = settings(OrgRole::Member, &kept)
        .read(&token(), an_org(MINE))
        .await
        .expect("a card");

    assert!(card.logo.is_none());
}

#[tokio::test]
async fn a_deployment_with_no_object_store_says_which_part_is_missing() {
    let kept = Kept::holding("Acme");

    let refused = settings(OrgRole::Admin, &kept)
        .put_logo(&token(), an_org(MINE), &a_png())
        .await
        .expect_err("there is no store");

    assert_eq!(refused, SettingError::Photos(PhotoError::NotOffered));
}

mod proptests {
    use proptest::prelude::*;

    use super::super::value_objects::MAX_ORG_NAME_BYTES;
    use super::{BadName, OrgName};

    fn a_printable_run(bytes: usize) -> impl Strategy<Value = String> {
        prop::collection::vec(0x21u8..=0x7eu8, 1..=bytes)
            .prop_map(|bytes| String::from_utf8(bytes).expect("ascii bytes"))
    }

    proptest! {
        #[test]
        fn a_name_the_constructor_admits_round_trips_once_padding_is_stripped(
            inner in a_printable_run(MAX_ORG_NAME_BYTES),
            leading in prop::collection::vec(prop_oneof![Just(' '), Just('\t')], 0..3),
            trailing in prop::collection::vec(prop_oneof![Just(' '), Just('\t')], 0..3),
        ) {
            let leading: String = leading.into_iter().collect();
            let trailing: String = trailing.into_iter().collect();
            let padded = format!("{leading}{inner}{trailing}");

            let name = OrgName::try_from(padded.as_str()).expect("a name within the ceiling");

            prop_assert_eq!(name.as_str(), inner.as_str());
        }

        #[test]
        fn a_name_one_byte_past_the_ceiling_is_refused_whatever_it_contains(
            over in prop::collection::vec(0x21u8..=0x7eu8, MAX_ORG_NAME_BYTES + 1)
                .prop_map(|bytes| String::from_utf8(bytes).expect("ascii bytes"))
        ) {
            prop_assert_eq!(
                OrgName::try_from(over.as_str()),
                Err(BadName::TooLong(MAX_ORG_NAME_BYTES))
            );
        }
    }
}

#[tokio::test]
async fn renaming_records_one_entry_naming_the_account_and_both_names() {
    let kept = Kept::holding("Acme");
    let notebook = Notebook::default();

    noting(OrgRole::Admin, &kept, &notebook)
        .rename(&token(), an_org(MINE), &a_name("Acme Payments"))
        .await
        .expect("an administrator may rename");

    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.org, an_org(MINE));
    assert_eq!(entry.action, Action::OrgRenamed);
    assert_eq!(
        entry.actor,
        Actor::Account(AccountId::new(Uuid::from_u128(9)))
    );
    assert_eq!(entry.target, Target::org(an_org(MINE)));
    assert_eq!(entry.detail.get("from"), Some(Some("Acme")));
    assert_eq!(entry.detail.get("to"), Some(Some("Acme Payments")));
}

#[tokio::test]
async fn a_refused_rename_records_nothing() {
    let kept = Kept::holding("Acme");
    let notebook = Notebook::default();

    let refused = noting(OrgRole::Member, &kept, &notebook)
        .rename(&token(), an_org(MINE), &a_name("Mine Now"))
        .await;

    assert_eq!(refused, Err(SettingError::NotYoursToChange));
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn keeping_then_dropping_a_logo_records_each_change_once() {
    let kept = Kept::holding("Acme");
    let shelf = Shelf::default();
    let notebook = Notebook::default();
    let settings = marked_noting(OrgRole::Admin, &kept, &shelf, &notebook);

    settings
        .put_logo(&token(), an_org(MINE), &a_png())
        .await
        .expect("a kept logo");
    settings
        .remove_logo(&token(), an_org(MINE))
        .await
        .expect("a dropped logo");
    settings
        .remove_logo(&token(), an_org(MINE))
        .await
        .expect("nothing left to drop");

    let actions: Vec<Action> = notebook
        .entries()
        .iter()
        .map(|entry| entry.action)
        .collect();
    assert_eq!(actions, vec![Action::OrgLogoSet, Action::OrgLogoRemoved]);
    assert_eq!(
        notebook.entries()[0].detail.get("kind"),
        Some(Some("image/png"))
    );
}
