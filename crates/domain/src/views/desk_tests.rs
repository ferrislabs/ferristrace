use std::sync::Mutex;

use uuid::Uuid;

use super::desk::ViewDesk;
use super::entities::{NewView, View, ViewChange};
use super::errors::ViewError;
use super::ports::{Creation, ExplorerViews, Saving};
use super::value_objects::{ViewDefinition, ViewId, ViewName, ViewSignal, Visibility};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::TenantId;

const MINE: u128 = 1;
const THEIRS: u128 = 2;
const ALICE: u128 = 9;
const BOB: u128 = 10;
const BOSS: u128 = 11;
const STRANGER: u128 = 12;

fn org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn token(who: &str) -> SessionToken {
    SessionToken::for_subject(who)
}

struct People;

impl SessionStore for People {
    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        let seed = match token.identity().subject.as_str() {
            "alice" => ALICE,
            "bob" => BOB,
            "boss" => BOSS,
            "stranger" => STRANGER,
            _ => return Err(SessionError::Unknown),
        };
        Ok(Session {
            account: account(seed),
            email: Email::try_from("person@example.com").expect("an address"),
            name: None,
            photo: None,
            org: None,
            expires_at: UnixSeconds::new(0),
        })
    }
}

struct Roles;

impl Memberships for Roles {
    async fn of(&self, who: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        let role = match who {
            id if id == account(BOSS) => OrgRole::Admin,
            id if id == account(ALICE) || id == account(BOB) => OrgRole::Member,
            _ => return Ok(Vec::new()),
        };
        Ok(vec![Membership {
            org: org(MINE),
            name: "Acme".into(),
            role,
        }])
    }
}

#[derive(Default)]
struct Shelf {
    kept: Mutex<Vec<(TenantId, View)>>,
}

impl Shelf {
    fn clashes(
        &self,
        org: TenantId,
        name: &str,
        visibility: Visibility,
        by: Option<AccountId>,
        except: Option<ViewId>,
    ) -> bool {
        self.kept
            .lock()
            .expect("the lock")
            .iter()
            .any(|(held_org, held)| {
                *held_org == org
                    && &*held.name == name
                    && Some(held.id) != except
                    && held.visibility == visibility
                    && (visibility == Visibility::Org || held.created_by == by)
            })
    }
}

impl ExplorerViews for Shelf {
    async fn visible(
        &self,
        org: TenantId,
        signal: ViewSignal,
        to: AccountId,
    ) -> Result<Vec<View>, StoreError> {
        Ok(self
            .kept
            .lock()
            .expect("the lock")
            .iter()
            .filter(|(held_org, held)| {
                *held_org == org
                    && held.signal == signal
                    && (held.visibility == Visibility::Org || held.created_by == Some(to))
            })
            .map(|(_, held)| held.clone())
            .collect())
    }

    async fn find(&self, org: TenantId, id: ViewId) -> Result<Option<View>, StoreError> {
        Ok(self
            .kept
            .lock()
            .expect("the lock")
            .iter()
            .find(|(held_org, held)| *held_org == org && held.id == id)
            .map(|(_, held)| held.clone()))
    }

    async fn create(
        &self,
        org: TenantId,
        by: AccountId,
        view: &NewView,
    ) -> Result<Creation, StoreError> {
        if self.clashes(org, view.name.as_str(), view.visibility, Some(by), None) {
            return Ok(Creation::NameTaken);
        }
        let mut kept = self.kept.lock().expect("the lock");
        let made = View {
            id: ViewId::new(Uuid::from_u128(1000 + kept.len() as u128)),
            signal: view.signal,
            name: view.name.as_str().into(),
            visibility: view.visibility,
            definition: view.definition.as_str().into(),
            created_by: Some(by),
            created_at_unix_nanos: 1,
            updated_at_unix_nanos: 1,
        };
        kept.push((org, made.clone()));
        Ok(Creation::Made(made))
    }

    async fn change(
        &self,
        org: TenantId,
        id: ViewId,
        change: &ViewChange,
    ) -> Result<Saving, StoreError> {
        let Some(held) = self.find(org, id).await? else {
            return Ok(Saving::Missing);
        };
        let visibility = change.visibility.unwrap_or(held.visibility);
        if self.clashes(
            org,
            change.name.as_str(),
            visibility,
            held.created_by,
            Some(id),
        ) {
            return Ok(Saving::NameTaken);
        }
        let mut kept = self.kept.lock().expect("the lock");
        if let Some((_, row)) = kept.iter_mut().find(|(_, row)| row.id == id) {
            row.name = change.name.as_str().into();
            row.definition = change.definition.as_str().into();
            row.visibility = visibility;
        }
        Ok(Saving::Saved)
    }

    async fn delete(&self, org: TenantId, id: ViewId) -> Result<bool, StoreError> {
        let mut kept = self.kept.lock().expect("the lock");
        let before = kept.len();
        kept.retain(|(held_org, held)| !(*held_org == org && held.id == id));
        Ok(kept.len() < before)
    }
}

fn desk(shelf: &Shelf) -> ViewDesk<People, Roles, &Shelf> {
    ViewDesk::new(Whoami::new(People, Roles), shelf)
}

fn new_view(name: &str, visibility: Visibility) -> NewView {
    NewView {
        signal: ViewSignal::Traces,
        name: ViewName::try_from(name).expect("a name"),
        visibility,
        definition: ViewDefinition::try_from(r#"{"q":[]}"#).expect("a definition"),
    }
}

fn change(name: &str, visibility: Option<Visibility>) -> ViewChange {
    ViewChange {
        name: ViewName::try_from(name).expect("a name"),
        definition: ViewDefinition::try_from(r#"{"q":[1]}"#).expect("a definition"),
        visibility,
    }
}

async fn made(
    desk: &ViewDesk<People, Roles, &Shelf>,
    who: &str,
    name: &str,
    visibility: Visibility,
) -> View {
    desk.create(&token(who), org(MINE), &new_view(name, visibility))
        .await
        .expect("the view is created")
}

#[tokio::test]
async fn a_member_sees_the_organisation_views_and_its_own_private_ones_only() {
    let shelf = Shelf::default();
    let desk = desk(&shelf);
    made(&desk, "alice", "shared", Visibility::Org).await;
    made(&desk, "alice", "mine", Visibility::Private).await;
    made(&desk, "bob", "bobs", Visibility::Private).await;

    let alice: Vec<String> = desk
        .list(&token("alice"), org(MINE), ViewSignal::Traces)
        .await
        .expect("a list")
        .into_iter()
        .map(|v| v.name.into_string())
        .collect();
    let bob: Vec<String> = desk
        .list(&token("bob"), org(MINE), ViewSignal::Traces)
        .await
        .expect("a list")
        .into_iter()
        .map(|v| v.name.into_string())
        .collect();

    assert_eq!(alice, vec!["shared", "mine"]);
    assert_eq!(bob, vec!["shared", "bobs"]);
}

#[tokio::test]
async fn someone_outside_the_organisation_neither_lists_nor_creates() {
    let shelf = Shelf::default();
    let desk = desk(&shelf);

    let listed = desk
        .list(&token("stranger"), org(MINE), ViewSignal::Traces)
        .await;
    let created = desk
        .create(
            &token("stranger"),
            org(MINE),
            &new_view("x", Visibility::Org),
        )
        .await;
    let elsewhere = desk
        .list(&token("alice"), org(THEIRS), ViewSignal::Traces)
        .await;

    assert_eq!(listed, Err(ViewError::NotYours));
    assert_eq!(created, Err(ViewError::NotYours));
    assert_eq!(elsewhere, Err(ViewError::NotYours));
}

#[tokio::test]
async fn a_token_nobody_holds_is_refused() {
    let shelf = Shelf::default();

    let listed = desk(&shelf)
        .list(&token("ghost"), org(MINE), ViewSignal::Traces)
        .await;

    assert_eq!(listed, Err(ViewError::NoSession));
}

#[tokio::test]
async fn an_organisation_name_is_unique_but_two_people_may_each_keep_a_private_one() {
    let shelf = Shelf::default();
    let desk = desk(&shelf);
    made(&desk, "alice", "slow", Visibility::Org).await;
    made(&desk, "alice", "slow", Visibility::Private).await;
    made(&desk, "bob", "slow", Visibility::Private).await;

    let again = desk
        .create(&token("bob"), org(MINE), &new_view("slow", Visibility::Org))
        .await;
    let twice = desk
        .create(
            &token("alice"),
            org(MINE),
            &new_view("slow", Visibility::Private),
        )
        .await;

    assert_eq!(again, Err(ViewError::NameTaken));
    assert_eq!(twice, Err(ViewError::NameTaken));
}

#[tokio::test]
async fn the_creator_changes_its_view() {
    let shelf = Shelf::default();
    let desk = desk(&shelf);
    let view = made(&desk, "alice", "slow", Visibility::Org).await;

    let saved = desk
        .change(&token("alice"), org(MINE), view.id, &change("slower", None))
        .await
        .expect("changed");

    assert_eq!(&*saved.name, "slower");
    assert_eq!(&*saved.definition, r#"{"q":[1]}"#);
}

#[tokio::test]
async fn another_member_cannot_change_or_delete_an_organisation_view_but_an_administrator_can() {
    let shelf = Shelf::default();
    let desk = desk(&shelf);
    let view = made(&desk, "alice", "slow", Visibility::Org).await;

    let edited = desk
        .change(&token("bob"), org(MINE), view.id, &change("x", None))
        .await;
    let deleted = desk.delete(&token("bob"), org(MINE), view.id).await;
    let by_boss = desk
        .change(&token("boss"), org(MINE), view.id, &change("by boss", None))
        .await;
    let removed = desk.delete(&token("boss"), org(MINE), view.id).await;

    assert_eq!(edited, Err(ViewError::NotYoursToChange));
    assert_eq!(deleted, Err(ViewError::NotYoursToChange));
    assert_eq!(
        by_boss.map(|v| v.name.into_string()),
        Ok("by boss".to_owned())
    );
    assert_eq!(removed, Ok(()));
    assert_eq!(
        desk.delete(&token("alice"), org(MINE), view.id).await,
        Err(ViewError::NoSuchView)
    );
}

#[tokio::test]
async fn a_private_view_does_not_exist_for_anyone_else_not_even_an_administrator() {
    let shelf = Shelf::default();
    let desk = desk(&shelf);
    let view = made(&desk, "alice", "mine", Visibility::Private).await;

    let by_boss = desk
        .change(&token("boss"), org(MINE), view.id, &change("x", None))
        .await;
    let by_bob = desk.delete(&token("bob"), org(MINE), view.id).await;

    assert_eq!(by_boss, Err(ViewError::NoSuchView));
    assert_eq!(by_bob, Err(ViewError::NoSuchView));
}

#[tokio::test]
async fn only_the_creator_changes_who_sees_a_view() {
    let shelf = Shelf::default();
    let desk = desk(&shelf);
    let view = made(&desk, "alice", "slow", Visibility::Org).await;

    let by_boss = desk
        .change(
            &token("boss"),
            org(MINE),
            view.id,
            &change("slow", Some(Visibility::Private)),
        )
        .await;
    let by_alice = desk
        .change(
            &token("alice"),
            org(MINE),
            view.id,
            &change("slow", Some(Visibility::Private)),
        )
        .await
        .expect("changed");
    let kept_as_is = desk
        .change(
            &token("boss"),
            org(MINE),
            view.id,
            &change("still", Some(Visibility::Private)),
        )
        .await;

    assert_eq!(by_boss, Err(ViewError::NotYoursToShare));
    assert_eq!(by_alice.visibility, Visibility::Private);
    assert_eq!(kept_as_is, Err(ViewError::NoSuchView));
}

#[tokio::test]
async fn a_change_cannot_take_the_name_of_another_view() {
    let shelf = Shelf::default();
    let desk = desk(&shelf);
    made(&desk, "alice", "slow", Visibility::Org).await;
    let other = made(&desk, "alice", "fast", Visibility::Org).await;

    let renamed = desk
        .change(&token("alice"), org(MINE), other.id, &change("slow", None))
        .await;

    assert_eq!(renamed, Err(ViewError::NameTaken));
}

#[tokio::test]
async fn a_view_of_another_organisation_is_never_found() {
    let shelf = Shelf::default();
    let desk = desk(&shelf);
    let view = made(&desk, "alice", "slow", Visibility::Org).await;

    let there = desk.delete(&token("alice"), org(THEIRS), view.id).await;

    assert_eq!(there, Err(ViewError::NotYours));
}
