use core::fmt;
use core::future::Future;

use uuid::Uuid;

use crate::explore::ReadError;
use crate::membership::{Memberships, OrgRole, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::session::SessionStore;
use crate::teams::TeamId;
use crate::tenant::TenantId;

const MAX_NAME_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadProjectName {
    #[error("a project needs a name")]
    Blank,
    #[error("a project name is at most {0} bytes")]
    TooLong(usize),
    #[error("a project name holds lowercase letters, digits and dashes, and starts with one")]
    Shape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectName(Box<str>);

impl ProjectName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ProjectName {
    type Error = BadProjectName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim().to_ascii_lowercase();
        if kept.len() > MAX_NAME_BYTES {
            return Err(BadProjectName::TooLong(MAX_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadProjectName::Blank);
        }
        let shaped = kept
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !kept.starts_with('-')
            && !kept.ends_with('-');
        if shaped {
            Ok(Self(kept.into()))
        } else {
            Err(BadProjectName::Shape)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(Uuid);

impl ProjectId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: ProjectId,
    pub org: TenantId,
    pub name: Box<str>,
    pub created_at_unix_nanos: u64,
    pub team: Option<OwnerTeam>,
    pub keys: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerTeam {
    pub id: TeamId,
    pub name: Box<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Created {
    Done,
    Taken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renamed {
    Done,
    Taken,
    Gone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assigned {
    Done,
    Gone,
    NoSuchTeam,
}

pub trait Projects {
    fn of(&self, tenant: TenantId) -> impl Future<Output = Result<Vec<Project>, ReadError>> + Send;

    fn create(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Created, ReadError>> + Send;

    fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Renamed, ReadError>> + Send;

    fn forget(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;

    fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> impl Future<Output = Result<Assigned, ReadError>> + Send;
}

impl<T: Projects + Sync> Projects for &T {
    fn of(&self, tenant: TenantId) -> impl Future<Output = Result<Vec<Project>, ReadError>> + Send {
        (*self).of(tenant)
    }

    fn create(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Created, ReadError>> + Send {
        (*self).create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Renamed, ReadError>> + Send {
        (*self).rename(org, project, name)
    }

    fn forget(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).forget(org, project)
    }

    fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> impl Future<Output = Result<Assigned, ReadError>> + Send {
        (*self).assign(org, project, team)
    }
}

impl<T: Projects + Send + Sync> Projects for std::sync::Arc<T> {
    fn of(&self, tenant: TenantId) -> impl Future<Output = Result<Vec<Project>, ReadError>> + Send {
        self.as_ref().of(tenant)
    }

    fn create(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Created, ReadError>> + Send {
        self.as_ref().create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> impl Future<Output = Result<Renamed, ReadError>> + Send {
        self.as_ref().rename(org, project, name)
    }

    fn forget(
        &self,
        org: TenantId,
        project: ProjectId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().forget(org, project)
    }

    fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> impl Future<Output = Result<Assigned, ReadError>> + Send {
        self.as_ref().assign(org, project, team)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CatalogueError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("the projects of an organisation are an administrator's to arrange")]
    NotYoursToChange,
    #[error("no project of this organisation carries that id")]
    NoSuchProject,
    #[error("no team of this organisation carries that id")]
    NoSuchTeam,
    #[error("this organisation already holds a project under that name")]
    NameTaken,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for CatalogueError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadError> for CatalogueError {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug)]
pub struct Catalogue<S, G, P> {
    whoami: Whoami<S, G>,
    projects: P,
}

impl<S, G, P> Catalogue<S, G, P>
where
    S: SessionStore,
    G: Memberships,
    P: Projects,
{
    pub fn new(whoami: Whoami<S, G>, projects: P) -> Self {
        Self { whoami, projects }
    }

    pub async fn projects(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Member).await?;
        Ok(self.projects.of(org).await?)
    }

    pub async fn create(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Admin).await?;
        match self.projects.create(org, name).await? {
            Created::Done => self.listed(org).await,
            Created::Taken => Err(CatalogueError::NameTaken),
        }
    }

    pub async fn rename(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Admin).await?;
        match self.projects.rename(org, project, name).await? {
            Renamed::Done => self.listed(org).await,
            Renamed::Taken => Err(CatalogueError::NameTaken),
            Renamed::Gone => Err(CatalogueError::NoSuchProject),
        }
    }

    pub async fn forget(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Admin).await?;
        if self.projects.forget(org, project).await? {
            self.listed(org).await
        } else {
            Err(CatalogueError::NoSuchProject)
        }
    }

    pub async fn assign(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> Result<Vec<Project>, CatalogueError> {
        self.standing(token, org, OrgRole::Admin).await?;
        match self.projects.assign(org, project, team).await? {
            Assigned::Done => self.listed(org).await,
            Assigned::Gone => Err(CatalogueError::NoSuchProject),
            Assigned::NoSuchTeam => Err(CatalogueError::NoSuchTeam),
        }
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
        floor: OrgRole,
    ) -> Result<(), CatalogueError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(CatalogueError::NotYours),
            Some(role) if !role.at_least(floor) => Err(CatalogueError::NotYoursToChange),
            Some(_) => Ok(()),
        }
    }

    async fn listed(&self, org: TenantId) -> Result<Vec<Project>, CatalogueError> {
        Ok(self.projects.of(org).await?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use uuid::Uuid;

    use super::{
        Assigned, BadProjectName, Catalogue, CatalogueError, Created, MAX_NAME_BYTES, Project,
        ProjectId, ProjectName, Projects, Renamed,
    };
    use crate::clock::UnixSeconds;
    use crate::credential::Email;
    use crate::explore::ReadError;
    use crate::identity::AccountId;
    use crate::membership::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
    use crate::secret::SessionToken;
    use crate::session::{Agent, Session, SessionError, SessionStore};
    use crate::teams::TeamId;
    use crate::tenant::TenantId;

    const TOKEN: &str = "ct_ss_live";
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
        async fn of(&self, tenant: TenantId) -> Result<Vec<Project>, ReadError> {
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

        async fn create(&self, _org: TenantId, name: &ProjectName) -> Result<Created, ReadError> {
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
        ) -> Result<Renamed, ReadError> {
            self.written
                .lock()
                .expect("the lock")
                .renamed
                .push((project, name.as_str().to_owned()));
            Ok(self.renamed)
        }

        async fn forget(&self, _org: TenantId, project: ProjectId) -> Result<bool, ReadError> {
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
        ) -> Result<Assigned, ReadError> {
            self.written
                .lock()
                .expect("the lock")
                .assigned
                .push((project, team));
            Ok(self.assigned)
        }
    }

    fn cataloguing(role: OrgRole, held: &Held) -> Catalogue<OneSession, MemberOfMine, &Held> {
        Catalogue::new(Whoami::new(OneSession, MemberOfMine(role)), held)
    }

    fn token() -> SessionToken {
        SessionToken::from_plaintext(TOKEN)
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
            .projects(&SessionToken::from_plaintext("ct_ss_nope"), an_org(MINE))
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
}
