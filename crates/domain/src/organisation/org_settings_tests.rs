use std::sync::Mutex;
use uuid::Uuid;

use super::entities::{Membership, Organisation};
use super::errors::{BadName, SettingError, WhoamiError};
use super::org_settings::OrgSettings;
use super::ports::{Memberships, Orgs};
use super::value_objects::{MAX_ORG_NAME_BYTES, OrgName, OrgRole, OrgTimezone};
use super::whoami::Whoami;
use crate::audit::{Action, Actor, Target};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Email, Session, SessionError, SessionStore};
use crate::profile::{NoPhotos, Photo, PhotoError, PhotoKind, Photos, Pictured, Portrait};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::{RetentionDays, TenantId};
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
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.identity().subject.as_str() == TOKEN {
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
    retention: Mutex<Option<RetentionDays>>,
    timezone: Mutex<OrgTimezone>,
    cap: Option<RetentionDays>,
}

impl Kept {
    fn holding(name: &str) -> Self {
        Self {
            name: Mutex::new(Some(name.to_owned())),
            renames: Mutex::default(),
            logo: Mutex::default(),
            retention: Mutex::default(),
            timezone: Mutex::default(),
            cap: None,
        }
    }

    fn gone() -> Self {
        Self {
            name: Mutex::new(None),
            renames: Mutex::default(),
            logo: Mutex::default(),
            retention: Mutex::default(),
            timezone: Mutex::default(),
            cap: None,
        }
    }

    fn capped_at(mut self, days: u32) -> Self {
        self.cap = Some(days_of(days));
        self
    }

    fn retention(&self) -> Option<RetentionDays> {
        *self.retention.lock().expect("the lock")
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
                retention: self.retention(),
                retention_cap: self.cap,
                timezone: self.timezone.lock().expect("the lock").clone(),
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

    async fn set_retention(
        &self,
        _org: TenantId,
        retention: Option<RetentionDays>,
    ) -> Result<(), StoreError> {
        *self.retention.lock().expect("the lock") = retention;
        Ok(())
    }

    async fn set_timezone(&self, _org: TenantId, timezone: &OrgTimezone) -> Result<(), StoreError> {
        *self.timezone.lock().expect("the lock") = timezone.clone();
        Ok(())
    }
}

fn days_of(days: u32) -> RetentionDays {
    RetentionDays::try_from(days).expect("an admitted retention")
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
    SessionToken::for_subject(TOKEN)
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
            retention: None,
            retention_cap: None,
            timezone: OrgTimezone::utc(),
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
        .read(&SessionToken::for_subject("ft_ss_nope"), an_org(MINE))
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

#[tokio::test]
async fn an_owner_sets_seven_days_and_one_entry_says_so() {
    let kept = Kept::holding("Acme");
    let notebook = Notebook::default();

    let card = noting(OrgRole::Owner, &kept, &notebook)
        .set_retention(&token(), an_org(MINE), Some(days_of(7)))
        .await
        .expect("an owner may set it");

    assert_eq!(card.retention, Some(days_of(7)));
    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::OrgRetentionSet);
    assert_eq!(entries[0].target, Target::org(an_org(MINE)));
    assert_eq!(entries[0].detail.get("from"), Some(Some("default")));
    assert_eq!(entries[0].detail.get("to"), Some(Some("7")));
}

#[tokio::test]
async fn an_administrator_may_not_set_it_and_nothing_is_recorded() {
    let kept = Kept::holding("Acme");
    let notebook = Notebook::default();

    for role in [OrgRole::Admin, OrgRole::Member] {
        let refused = noting(role, &kept, &notebook)
            .set_retention(&token(), an_org(MINE), Some(days_of(7)))
            .await;

        assert_eq!(refused, Err(SettingError::OwnersOnly));
    }

    assert_eq!(kept.retention(), None);
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn setting_it_on_an_organisation_this_session_is_no_part_of_says_no_such_thing() {
    let kept = Kept::holding("Acme");
    let notebook = Notebook::default();

    let refused = noting(OrgRole::Owner, &kept, &notebook)
        .set_retention(&token(), an_org(THEIRS), Some(days_of(7)))
        .await;

    assert_eq!(refused, Err(SettingError::NotYours));
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn a_value_above_the_plan_cap_is_refused_and_nothing_is_recorded() {
    let kept = Kept::holding("Acme").capped_at(30);
    let notebook = Notebook::default();

    let refused = noting(OrgRole::Owner, &kept, &notebook)
        .set_retention(&token(), an_org(MINE), Some(days_of(31)))
        .await;

    assert_eq!(refused, Err(SettingError::AboveCap { cap: days_of(30) }));
    assert_eq!(kept.retention(), None);
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn a_value_at_the_plan_cap_is_kept() {
    let kept = Kept::holding("Acme").capped_at(30);

    let card = settings(OrgRole::Owner, &kept)
        .set_retention(&token(), an_org(MINE), Some(days_of(30)))
        .await
        .expect("the cap itself is allowed");

    assert_eq!(card.retention, Some(days_of(30)));
    assert_eq!(card.retention_cap, Some(days_of(30)));
}

#[tokio::test]
async fn none_resets_it_to_the_default_and_the_entry_names_both_ends() {
    let kept = Kept::holding("Acme");
    let notebook = Notebook::default();
    noting(OrgRole::Owner, &kept, &notebook)
        .set_retention(&token(), an_org(MINE), Some(days_of(7)))
        .await
        .expect("set");

    let card = noting(OrgRole::Owner, &kept, &notebook)
        .set_retention(&token(), an_org(MINE), None)
        .await
        .expect("reset");

    assert_eq!(card.retention, None);
    let entries = notebook.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].detail.get("from"), Some(Some("7")));
    assert_eq!(entries[1].detail.get("to"), Some(Some("default")));
}

#[test]
fn a_retention_is_between_one_day_and_ten_years() {
    assert!(RetentionDays::try_from(0).is_err());
    assert!(RetentionDays::try_from(1).is_ok());
    assert!(RetentionDays::try_from(3650).is_ok());
    assert!(RetentionDays::try_from(3651).is_err());
    assert_eq!(days_of(7).seconds(), 7 * 24 * 60 * 60);
    assert_eq!(RetentionDays::saturating(0).get(), 1);
    assert_eq!(RetentionDays::saturating(u32::MAX).get(), 3650);
}

fn paris() -> OrgTimezone {
    OrgTimezone::try_from("Europe/Paris").expect("an IANA name")
}

#[test]
fn a_time_zone_is_an_iana_name_and_nothing_else() {
    assert_eq!(paris().as_str(), "Europe/Paris");
    assert_eq!(OrgTimezone::default().as_str(), "UTC");
    assert_eq!(
        OrgTimezone::try_from("  Asia/Tokyo ").map(|zone| zone.to_string()),
        Ok("Asia/Tokyo".to_owned())
    );
    for bad in ["", "   ", "Mars/Olympus", "CET+1"] {
        assert!(OrgTimezone::try_from(bad).is_err(), "{bad:?}");
    }
    assert!(OrgTimezone::try_from("x".repeat(65).as_str()).is_err());
}

#[tokio::test]
async fn a_new_card_reads_utc() {
    let kept = Kept::holding("Acme");

    let card = settings(OrgRole::Member, &kept)
        .read(&token(), an_org(MINE))
        .await
        .expect("a member may read");

    assert_eq!(card.timezone, OrgTimezone::utc());
}

#[tokio::test]
async fn an_administrator_sets_the_time_zone_and_the_change_is_audited_with_both_sides() {
    let kept = Kept::holding("Acme");
    let notebook = Notebook::default();

    let card = noting(OrgRole::Admin, &kept, &notebook)
        .set_timezone(&token(), an_org(MINE), &paris())
        .await
        .expect("an administrator may set it");

    assert_eq!(card.timezone, paris());
    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::OrgTimezoneSet);
    assert_eq!(entries[0].target, Target::org(an_org(MINE)));
    assert_eq!(entries[0].detail.get("from"), Some(Some("UTC")));
    assert_eq!(entries[0].detail.get("to"), Some(Some("Europe/Paris")));
}

#[tokio::test]
async fn a_member_cannot_set_the_time_zone_and_nothing_is_written() {
    let kept = Kept::holding("Acme");
    let notebook = Notebook::default();

    let refused = noting(OrgRole::Member, &kept, &notebook)
        .set_timezone(&token(), an_org(MINE), &paris())
        .await;

    assert_eq!(refused, Err(SettingError::NotYoursToChange));
    assert!(notebook.entries().is_empty());
    assert_eq!(*kept.timezone.lock().expect("the lock"), OrgTimezone::utc());
}

#[tokio::test]
async fn the_time_zone_of_another_organisation_is_not_ours_to_set() {
    let kept = Kept::holding("Acme");

    let refused = settings(OrgRole::Owner, &kept)
        .set_timezone(&token(), an_org(THEIRS), &paris())
        .await;

    assert_eq!(refused, Err(SettingError::NotYours));
}

#[tokio::test]
async fn setting_the_zone_it_already_has_writes_nothing() {
    let kept = Kept::holding("Acme");
    let notebook = Notebook::default();

    noting(OrgRole::Admin, &kept, &notebook)
        .set_timezone(&token(), an_org(MINE), &OrgTimezone::utc())
        .await
        .expect("a no-op");

    assert!(notebook.entries().is_empty());
}
