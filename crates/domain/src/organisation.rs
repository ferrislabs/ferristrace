use core::future::Future;

use crate::explore::ReadError;
use crate::membership::{Memberships, OrgRole, Whoami, WhoamiError};
use crate::photo::{Photo, PhotoError, PhotoKind, Photos, Pictured, Portrait};
use crate::secret::SessionToken;
use crate::session::SessionStore;
use crate::tenant::TenantId;

const MAX_NAME_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadName {
    #[error("an organisation needs a name")]
    Blank,
    #[error("an organisation name is at most {0} bytes")]
    TooLong(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgName(Box<str>);

impl OrgName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for OrgName {
    type Error = BadName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim();
        if kept.len() > MAX_NAME_BYTES {
            return Err(BadName::TooLong(MAX_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadName::Blank);
        }
        Ok(Self(kept.into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organisation {
    pub id: TenantId,
    pub name: Box<str>,
    pub plan: Box<str>,
    pub created_at_unix_nanos: u64,
    pub logo: Option<Portrait>,
}

pub trait Orgs {
    fn only(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<Organisation>, ReadError>> + Send;

    fn rename(
        &self,
        org: TenantId,
        name: &OrgName,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;

    fn set_logo(
        &self,
        org: TenantId,
        logo: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send;
}

impl<T: Orgs + Sync> Orgs for &T {
    fn only(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<Organisation>, ReadError>> + Send {
        (*self).only(org)
    }

    fn rename(
        &self,
        org: TenantId,
        name: &OrgName,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).rename(org, name)
    }

    fn set_logo(
        &self,
        org: TenantId,
        logo: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        (*self).set_logo(org, logo)
    }
}

impl<T: Orgs + Send + Sync> Orgs for std::sync::Arc<T> {
    fn only(
        &self,
        org: TenantId,
    ) -> impl Future<Output = Result<Option<Organisation>, ReadError>> + Send {
        self.as_ref().only(org)
    }

    fn rename(
        &self,
        org: TenantId,
        name: &OrgName,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        self.as_ref().rename(org, name)
    }

    fn set_logo(
        &self,
        org: TenantId,
        logo: Option<&Portrait>,
    ) -> impl Future<Output = Result<(), ReadError>> + Send {
        self.as_ref().set_logo(org, logo)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("changing an organisation is an administrator's to do")]
    NotYoursToChange,
    #[error("no organisation carries that id any more")]
    Gone,
    #[error("that organisation keeps no logo")]
    NoLogo,
    #[error(transparent)]
    Photos(PhotoError),
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for SettingError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadError> for SettingError {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<PhotoError> for SettingError {
    fn from(error: PhotoError) -> Self {
        match error {
            PhotoError::NotOffered => Self::Photos(PhotoError::NotOffered),
            PhotoError::Unreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug)]
pub struct OrgSettings<S, G, O, P> {
    whoami: Whoami<S, G>,
    orgs: O,
    photos: P,
}

impl<S, G, O, P> OrgSettings<S, G, O, P>
where
    S: SessionStore,
    G: Memberships,
    O: Orgs,
    P: Photos,
{
    pub fn new(whoami: Whoami<S, G>, orgs: O, photos: P) -> Self {
        Self {
            whoami,
            orgs,
            photos,
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
        self.standing(token, org, OrgRole::Admin).await?;
        self.orgs.rename(org, name).await?;
        self.card(org).await
    }

    pub async fn put_logo(
        &self,
        token: &SessionToken,
        org: TenantId,
        logo: &Photo,
    ) -> Result<Organisation, SettingError> {
        self.standing(token, org, OrgRole::Admin).await?;
        let before = self.card(org).await?.logo;
        let kept = self.photos.put(Pictured::Org(org), logo).await?;
        self.orgs.set_logo(org, Some(&kept)).await?;
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
        self.standing(token, org, OrgRole::Admin).await?;
        let Some(kept) = self.card(org).await?.logo else {
            return self.card(org).await;
        };
        self.orgs.set_logo(org, None).await?;
        let _ = self.photos.remove(Pictured::Org(org), &kept).await;
        self.card(org).await
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
    ) -> Result<(), SettingError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(SettingError::NotYours),
            Some(role) if !role.at_least(floor) => Err(SettingError::NotYoursToChange),
            Some(_) => Ok(()),
        }
    }

    async fn card(&self, org: TenantId) -> Result<Organisation, SettingError> {
        self.orgs.only(org).await?.ok_or(SettingError::Gone)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use uuid::Uuid;

    use super::{BadName, MAX_NAME_BYTES, OrgName, OrgSettings, Organisation, Orgs, SettingError};
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
        async fn only(&self, org: TenantId) -> Result<Option<Organisation>, ReadError> {
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

        async fn rename(&self, _org: TenantId, name: &OrgName) -> Result<(), ReadError> {
            self.renames
                .lock()
                .expect("the lock")
                .push(name.as_str().to_owned());
            *self.name.lock().expect("the lock") = Some(name.as_str().to_owned());
            Ok(())
        }

        async fn set_logo(&self, _org: TenantId, logo: Option<&Portrait>) -> Result<(), ReadError> {
            *self.logo.lock().expect("the lock") = logo.cloned();
            Ok(())
        }
    }

    fn settings(
        role: OrgRole,
        kept: &Kept,
    ) -> OrgSettings<OneSession, MemberOfMine, &Kept, NoPhotos> {
        OrgSettings::new(Whoami::new(OneSession, MemberOfMine(role)), kept, NoPhotos)
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
    ) -> OrgSettings<OneSession, MemberOfMine, &'a Kept, &'a Shelf> {
        OrgSettings::new(Whoami::new(OneSession, MemberOfMine(role)), kept, shelf)
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
            .read(&SessionToken::from_plaintext("ct_ss_nope"), an_org(MINE))
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
        let long = "a".repeat(MAX_NAME_BYTES + 1);

        assert_eq!(
            OrgName::try_from(long.as_str()),
            Err(BadName::TooLong(MAX_NAME_BYTES))
        );
        assert!(OrgName::try_from("a".repeat(MAX_NAME_BYTES).as_str()).is_ok());
    }

    #[test]
    fn the_ceiling_counts_bytes_rather_than_characters() {
        let wide = "\u{e9}".repeat(MAX_NAME_BYTES / 2 + 1);

        assert!(wide.chars().count() < MAX_NAME_BYTES);
        assert_eq!(
            OrgName::try_from(wide.as_str()),
            Err(BadName::TooLong(MAX_NAME_BYTES))
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

        use super::super::MAX_NAME_BYTES;
        use super::{BadName, OrgName};

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

                let name = OrgName::try_from(padded.as_str()).expect("a name within the ceiling");

                prop_assert_eq!(name.as_str(), inner.as_str());
            }

            #[test]
            fn a_name_one_byte_past_the_ceiling_is_refused_whatever_it_contains(
                over in prop::collection::vec(0x21u8..=0x7eu8, MAX_NAME_BYTES + 1)
                    .prop_map(|bytes| String::from_utf8(bytes).expect("ascii bytes"))
            ) {
                prop_assert_eq!(
                    OrgName::try_from(over.as_str()),
                    Err(BadName::TooLong(MAX_NAME_BYTES))
                );
            }
        }
    }
}
