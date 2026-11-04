use std::sync::Mutex;
use uuid::Uuid;

use super::catalogue_service::Catalogue;
use super::entities::Project;
use super::errors::{BadProjectName, CatalogueError};
use super::ports::Projects;
use super::value_objects::{Assigned, Created, MAX_NAME_BYTES, ProjectId, ProjectName, Renamed};
use crate::audit::{Action, Actor, Target};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, TeamId, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::TenantId;
use crate::testing::Notebook;

const TOKEN: &str = "ft_ss_live";
const MINE: u128 = 1;
const THEIRS: u128 = 2;
const STOREFRONT: u128 = 7;
const BACKEND: u128 = 11;
const CREATED: u64 = 1_700_000_000_000_000_000;

fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn a_project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn a_team(seed: u128) -> TeamId {
    TeamId::new(Uuid::from_u128(seed))
}

fn a_name(spelling: &str) -> ProjectName {
    ProjectName::try_from(spelling).expect("an admitted name")
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Written {
    asked: Vec<TenantId>,
    created: Vec<String>,
    renamed: Vec<(ProjectId, String)>,
    forgotten: Vec<ProjectId>,
    assigned: Vec<(ProjectId, Option<TeamId>)>,
}

#[derive(Debug)]
struct Held {
    written: Mutex<Written>,
    created: Created,
    renamed: Renamed,
    forgotten: bool,
    assigned: Assigned,
}

impl Default for Held {
    fn default() -> Self {
        Self {
            written: Mutex::default(),
            created: Created::Done,
            renamed: Renamed::Done,
            forgotten: true,
            assigned: Assigned::Done,
        }
    }
}

impl Held {
    fn written(&self) -> Written {
        self.written.lock().expect("the lock").clone()
    }
}

impl Projects for &Held {
    async fn of(&self, tenant: TenantId) -> Result<Vec<Project>, StoreError> {
        self.written.lock().expect("the lock").asked.push(tenant);
        Ok(vec![Project {
            id: a_project(STOREFRONT),
            org: tenant,
            name: "storefront".into(),
            created_at_unix_nanos: CREATED,
            team: None,
            keys: 2,
        }])
    }

    async fn create(&self, _org: TenantId, name: &ProjectName) -> Result<Created, StoreError> {
        self.written
            .lock()
            .expect("the lock")
            .created
            .push(name.as_str().to_owned());
        Ok(self.created)
    }

    async fn rename(
        &self,
        _org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> Result<Renamed, StoreError> {
        self.written
            .lock()
            .expect("the lock")
            .renamed
            .push((project, name.as_str().to_owned()));
        Ok(self.renamed)
    }

    async fn forget(&self, _org: TenantId, project: ProjectId) -> Result<bool, StoreError> {
        self.written
            .lock()
            .expect("the lock")
            .forgotten
            .push(project);
        Ok(self.forgotten)
    }

    async fn assign(
        &self,
        _org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> Result<Assigned, StoreError> {
        self.written
            .lock()
            .expect("the lock")
            .assigned
            .push((project, team));
        Ok(self.assigned)
    }
}

fn cataloguing(role: OrgRole, held: &Held) -> Catalogue<OneSession, MemberOfMine, &Held, Notebook> {
    noting(role, held, &Notebook::default())
}

fn noting<'a>(
    role: OrgRole,
    held: &'a Held,
    notebook: &Notebook,
) -> Catalogue<OneSession, MemberOfMine, &'a Held, Notebook> {
    Catalogue::new(
        Whoami::new(OneSession, MemberOfMine(role)),
        held,
        notebook.clone(),
    )
}

fn token() -> SessionToken {
    SessionToken::for_subject(TOKEN)
}

#[tokio::test]
async fn a_member_reads_the_projects_of_its_own_organisation() {
    let held = Held::default();

    let found = cataloguing(OrgRole::Member, &held)
        .projects(&token(), an_org(MINE))
        .await
        .expect("a member may read");

    assert_eq!(found.len(), 1);
    assert_eq!(&*found[0].name, "storefront");
    assert_eq!(found[0].keys, 2);
    assert_eq!(found[0].created_at_unix_nanos, CREATED);
    assert_eq!(held.written().asked, vec![an_org(MINE)]);
}

#[tokio::test]
async fn the_projects_of_another_organisation_are_refused_before_the_store_is_asked() {
    let held = Held::default();

    let refused = cataloguing(OrgRole::Owner, &held)
        .projects(&token(), an_org(THEIRS))
        .await;

    assert_eq!(refused, Err(CatalogueError::NotYours));
    assert_eq!(held.written(), Written::default());
}

#[tokio::test]
async fn a_token_no_session_carries_reads_nothing() {
    let held = Held::default();

    let refused = cataloguing(OrgRole::Owner, &held)
        .projects(&SessionToken::for_subject("ft_ss_nope"), an_org(MINE))
        .await;

    assert_eq!(refused, Err(CatalogueError::NoSession));
}

#[tokio::test]
async fn an_administrator_creates_a_project_and_is_answered_with_the_catalogue() {
    let held = Held::default();

    let found = cataloguing(OrgRole::Admin, &held)
        .create(&token(), an_org(MINE), &a_name("payments-eu"))
        .await
        .expect("an administrator may create a project");

    assert_eq!(held.written().created, vec!["payments-eu".to_owned()]);
    assert_eq!(found.len(), 1, "the answer carries the whole screen");
}

#[tokio::test]
async fn a_member_arranges_nothing() {
    let held = Held::default();

    let refused = cataloguing(OrgRole::Member, &held)
        .create(&token(), an_org(MINE), &a_name("payments-eu"))
        .await;

    assert_eq!(refused, Err(CatalogueError::NotYoursToChange));
    assert_eq!(held.written(), Written::default());
}

#[tokio::test]
async fn a_name_the_organisation_already_holds_is_refused() {
    let held = Held {
        created: Created::Taken,
        ..Held::default()
    };

    let refused = cataloguing(OrgRole::Admin, &held)
        .create(&token(), an_org(MINE), &a_name("storefront"))
        .await;

    assert_eq!(refused, Err(CatalogueError::NameTaken));
}

#[tokio::test]
async fn renaming_a_project_the_organisation_does_not_hold_says_so() {
    let held = Held {
        renamed: Renamed::Gone,
        ..Held::default()
    };

    let refused = cataloguing(OrgRole::Owner, &held)
        .rename(&token(), an_org(MINE), a_project(404), &a_name("gone"))
        .await;

    assert_eq!(refused, Err(CatalogueError::NoSuchProject));
}

#[tokio::test]
async fn renaming_a_project_onto_a_name_already_held_is_told_apart_from_that() {
    let held = Held {
        renamed: Renamed::Taken,
        ..Held::default()
    };

    let refused = cataloguing(OrgRole::Owner, &held)
        .rename(
            &token(),
            an_org(MINE),
            a_project(STOREFRONT),
            &a_name("payments-eu"),
        )
        .await;

    assert_eq!(refused, Err(CatalogueError::NameTaken));
}

#[tokio::test]
async fn an_administrator_forgets_a_project() {
    let held = Held::default();

    cataloguing(OrgRole::Admin, &held)
        .forget(&token(), an_org(MINE), a_project(STOREFRONT))
        .await
        .expect("an administrator may forget a project");

    assert_eq!(held.written().forgotten, vec![a_project(STOREFRONT)]);
}

#[tokio::test]
async fn forgetting_a_project_that_is_not_there_is_not_a_quiet_success() {
    let held = Held {
        forgotten: false,
        ..Held::default()
    };

    let refused = cataloguing(OrgRole::Owner, &held)
        .forget(&token(), an_org(MINE), a_project(404))
        .await;

    assert_eq!(refused, Err(CatalogueError::NoSuchProject));
}

#[tokio::test]
async fn a_project_is_handed_to_a_team_and_taken_back_again() {
    let held = Held::default();
    let cataloguing = cataloguing(OrgRole::Admin, &held);

    cataloguing
        .assign(
            &token(),
            an_org(MINE),
            a_project(STOREFRONT),
            Some(a_team(BACKEND)),
        )
        .await
        .expect("the team is handed the project");
    cataloguing
        .assign(&token(), an_org(MINE), a_project(STOREFRONT), None)
        .await
        .expect("and it can be taken back");

    assert_eq!(
        held.written().assigned,
        vec![
            (a_project(STOREFRONT), Some(a_team(BACKEND))),
            (a_project(STOREFRONT), None),
        ]
    );
}

#[tokio::test]
async fn a_team_of_another_organisation_cannot_be_handed_a_project() {
    let held = Held {
        assigned: Assigned::NoSuchTeam,
        ..Held::default()
    };

    let refused = cataloguing(OrgRole::Owner, &held)
        .assign(
            &token(),
            an_org(MINE),
            a_project(STOREFRONT),
            Some(a_team(404)),
        )
        .await;

    assert_eq!(refused, Err(CatalogueError::NoSuchTeam));
}

#[test]
fn a_name_is_lowered_rather_than_refused_for_its_case() {
    assert_eq!(a_name("  Storefront \n").as_str(), "storefront");
}

#[test]
fn a_name_holds_letters_digits_and_dashes_and_nothing_else() {
    assert!(ProjectName::try_from("payments-eu-2").is_ok());
    for refused in ["pay ments", "payments_eu", "payments.eu", "payments/eu"] {
        assert_eq!(
            ProjectName::try_from(refused),
            Err(BadProjectName::Shape),
            "{refused}"
        );
    }
}

#[test]
fn a_name_neither_starts_nor_ends_with_a_dash() {
    assert_eq!(
        ProjectName::try_from("-storefront"),
        Err(BadProjectName::Shape)
    );
    assert_eq!(
        ProjectName::try_from("storefront-"),
        Err(BadProjectName::Shape)
    );
}

#[test]
fn a_name_that_is_only_space_is_as_blank_as_an_empty_one() {
    assert_eq!(ProjectName::try_from(""), Err(BadProjectName::Blank));
    assert_eq!(ProjectName::try_from("   \t\n"), Err(BadProjectName::Blank));
}

#[test]
fn a_name_past_the_ceiling_is_refused() {
    let long = "a".repeat(MAX_NAME_BYTES + 1);

    assert_eq!(
        ProjectName::try_from(long.as_str()),
        Err(BadProjectName::TooLong(MAX_NAME_BYTES))
    );
    assert!(ProjectName::try_from("a".repeat(MAX_NAME_BYTES).as_str()).is_ok());
}

mod proptests {
    use proptest::prelude::*;

    use super::ProjectName;

    fn a_shaped_name() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9-]{0,20}[a-z0-9]"
    }

    proptest! {
        #[test]
        fn a_name_the_constructor_admits_round_trips_and_keeps_its_shape(
            chosen in a_shaped_name()
        ) {
            let name = ProjectName::try_from(chosen.as_str()).expect("a shaped name");

            prop_assert_eq!(name.as_str(), chosen.as_str());
            prop_assert!(
                name.as_str()
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            );
            prop_assert!(!name.as_str().starts_with('-'));
            prop_assert!(!name.as_str().ends_with('-'));
        }

        #[test]
        fn an_uppercase_name_is_admitted_lowered_rather_than_refused(
            chosen in a_shaped_name()
        ) {
            let upper = chosen.to_ascii_uppercase();

            let name = ProjectName::try_from(upper.as_str()).expect("a shaped name, differently cased");

            prop_assert_eq!(name.as_str(), chosen.as_str());
        }
    }
}

#[tokio::test]
async fn each_project_change_records_one_entry_naming_the_project() {
    let held = Held::default();
    let notebook = Notebook::default();
    let catalogue = noting(OrgRole::Admin, &held, &notebook);
    let org = an_org(MINE);
    let storefront = a_project(STOREFRONT);

    catalogue
        .create(
            &token(),
            org,
            &ProjectName::try_from("storefront").expect("a slug"),
        )
        .await
        .expect("a created project");
    catalogue
        .rename(
            &token(),
            org,
            storefront,
            &ProjectName::try_from("shop").expect("a slug"),
        )
        .await
        .expect("a renamed project");
    catalogue
        .assign(
            &token(),
            org,
            storefront,
            Some(TeamId::new(Uuid::from_u128(BACKEND))),
        )
        .await
        .expect("an assigned project");
    catalogue
        .forget(&token(), org, storefront)
        .await
        .expect("a forgotten project");

    let entries = notebook.entries();
    let actions: Vec<Action> = entries.iter().map(|entry| entry.action).collect();
    assert_eq!(
        actions,
        vec![
            Action::ProjectCreated,
            Action::ProjectRenamed,
            Action::ProjectAssigned,
            Action::ProjectDeleted,
        ]
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.target == Target::project(storefront))
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.actor == Actor::Account(AccountId::new(Uuid::from_u128(9))))
    );
    assert_eq!(entries[1].detail.get("from"), Some(Some("storefront")));
    assert_eq!(entries[1].detail.get("to"), Some(Some("shop")));
    let team = TeamId::new(Uuid::from_u128(BACKEND)).to_string();
    assert_eq!(entries[2].detail.get("team"), Some(Some(team.as_str())));
    assert_eq!(entries[3].detail.get("name"), Some(Some("storefront")));
}

#[tokio::test]
async fn a_refused_project_change_records_nothing() {
    let held = Held {
        created: Created::Taken,
        ..Held::default()
    };
    let notebook = Notebook::default();

    let refused = noting(OrgRole::Admin, &held, &notebook)
        .create(
            &token(),
            an_org(MINE),
            &ProjectName::try_from("storefront").expect("a slug"),
        )
        .await;

    assert_eq!(refused, Err(CatalogueError::NameTaken));
    assert!(notebook.entries().is_empty());
}
