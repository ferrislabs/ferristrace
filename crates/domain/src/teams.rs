use core::fmt;
use core::future::Future;

use uuid::Uuid;

use crate::catalogue::ProjectId;
use crate::explore::ReadError;
use crate::identity::AccountId;
use crate::membership::{Memberships, OrgRole, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::session::SessionStore;
use crate::tenant::TenantId;

const MAX_NAME_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BadTeamName {
    #[error("a team needs a name")]
    Blank,
    #[error("a team name is at most {0} bytes")]
    TooLong(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamName(Box<str>);

impl TeamName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for TeamName {
    type Error = BadTeamName;

    fn try_from(chosen: &str) -> Result<Self, Self::Error> {
        let kept = chosen.trim();
        if kept.len() > MAX_NAME_BYTES {
            return Err(BadTeamName::TooLong(MAX_NAME_BYTES));
        }
        if kept.is_empty() {
            return Err(BadTeamName::Blank);
        }
        Ok(Self(kept.into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TeamId(Uuid);

impl TeamId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for TeamId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    pub id: TeamId,
    pub name: Box<str>,
    pub created_at_unix_nanos: u64,
    pub members: Vec<AccountId>,
    pub projects: Vec<Owned>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owned {
    pub id: ProjectId,
    pub name: Box<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    Free,
    Taken,
}

pub trait Squads {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Team>, ReadError>> + Send;

    fn create(
        &self,
        org: TenantId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send;

    fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send;

    fn disband(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;

    fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;

    fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send;
}

impl<T: Squads + Sync> Squads for &T {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Team>, ReadError>> + Send {
        (*self).of(org)
    }

    fn create(
        &self,
        org: TenantId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send {
        (*self).create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send {
        (*self).rename(org, team, name)
    }

    fn disband(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).disband(org, team)
    }

    fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).seat(org, team, members)
    }

    fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        (*self).own(org, team, projects)
    }
}

impl<T: Squads + Send + Sync> Squads for std::sync::Arc<T> {
    fn of(&self, org: TenantId) -> impl Future<Output = Result<Vec<Team>, ReadError>> + Send {
        self.as_ref().of(org)
    }

    fn create(
        &self,
        org: TenantId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send {
        self.as_ref().create(org, name)
    }

    fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> impl Future<Output = Result<Named, ReadError>> + Send {
        self.as_ref().rename(org, team, name)
    }

    fn disband(
        &self,
        org: TenantId,
        team: TeamId,
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().disband(org, team)
    }

    fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().seat(org, team, members)
    }

    fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> impl Future<Output = Result<bool, ReadError>> + Send {
        self.as_ref().own(org, team, projects)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TeamError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("this session holds no membership of that organisation")]
    NotYours,
    #[error("teams are an administrator's to arrange")]
    NotYoursToChange,
    #[error("no team of this organisation carries that id")]
    NoSuchTeam,
    #[error("this organisation already holds a team under that name")]
    NameTaken,
    #[error("a store could not be reached")]
    StoreUnreachable,
}

impl From<WhoamiError> for TeamError {
    fn from(error: WhoamiError) -> Self {
        match error {
            WhoamiError::NoSession => Self::NoSession,
            WhoamiError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

impl From<ReadError> for TeamError {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug)]
pub struct Squad<S, G, Q> {
    whoami: Whoami<S, G>,
    squads: Q,
}

impl<S, G, Q> Squad<S, G, Q>
where
    S: SessionStore,
    G: Memberships,
    Q: Squads,
{
    pub fn new(whoami: Whoami<S, G>, squads: Q) -> Self {
        Self { whoami, squads }
    }

    pub async fn teams(&self, token: &SessionToken, org: TenantId) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Member).await?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn create(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &TeamName,
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        admitted(self.squads.create(org, name).await?)?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn rename(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        admitted(self.squads.rename(org, team, name).await?)?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn disband(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        found(self.squads.disband(org, team).await?)?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn seat(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        found(self.squads.seat(org, team, members).await?)?;
        Ok(self.squads.of(org).await?)
    }

    pub async fn own(
        &self,
        token: &SessionToken,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> Result<Vec<Team>, TeamError> {
        self.standing(token, org, OrgRole::Admin).await?;
        found(self.squads.own(org, team, projects).await?)?;
        Ok(self.squads.of(org).await?)
    }

    async fn standing(
        &self,
        token: &SessionToken,
        org: TenantId,
        floor: OrgRole,
    ) -> Result<(), TeamError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => Err(TeamError::NotYours),
            Some(role) if !role.at_least(floor) => Err(TeamError::NotYoursToChange),
            Some(_) => Ok(()),
        }
    }
}

fn admitted(named: Named) -> Result<(), TeamError> {
    match named {
        Named::Free => Ok(()),
        Named::Taken => Err(TeamError::NameTaken),
    }
}

fn found(there: bool) -> Result<(), TeamError> {
    if there {
        Ok(())
    } else {
        Err(TeamError::NoSuchTeam)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use uuid::Uuid;

    use super::{
        BadTeamName, MAX_NAME_BYTES, Named, Owned, Squad, Squads, Team, TeamError, TeamId, TeamName,
    };
    use crate::catalogue::ProjectId;
    use crate::clock::UnixSeconds;
    use crate::credential::Email;
    use crate::explore::ReadError;
    use crate::identity::AccountId;
    use crate::membership::{Membership, Memberships, OrgRole, Whoami, WhoamiError};
    use crate::secret::SessionToken;
    use crate::session::{Agent, Session, SessionError, SessionStore};
    use crate::tenant::TenantId;

    const TOKEN: &str = "ct_ss_live";
    const MINE: u128 = 1;
    const THEIRS: u128 = 2;
    const BACKEND: u128 = 11;
    const FOUNDED: u64 = 1_700_000_000_000_000_000;

    fn an_org(seed: u128) -> TenantId {
        TenantId::new(Uuid::from_u128(seed))
    }

    fn a_team(seed: u128) -> TeamId {
        TeamId::new(Uuid::from_u128(seed))
    }

    fn an_account(seed: u128) -> AccountId {
        AccountId::new(Uuid::from_u128(seed))
    }

    fn a_project(seed: u128) -> ProjectId {
        ProjectId::new(Uuid::from_u128(seed))
    }

    fn a_name(spelling: &str) -> TeamName {
        TeamName::try_from(spelling).expect("an admitted name")
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
                    account: an_account(9),
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
        created: Vec<String>,
        renamed: Vec<(TeamId, String)>,
        disbanded: Vec<TeamId>,
        seated: Vec<(TeamId, Vec<AccountId>)>,
        owned: Vec<(TeamId, Vec<ProjectId>)>,
    }

    #[derive(Debug)]
    struct Held {
        teams: Mutex<Vec<Team>>,
        written: Mutex<Written>,
        taken: bool,
    }

    impl Held {
        fn of(teams: Vec<Team>) -> Self {
            Self {
                teams: Mutex::new(teams),
                written: Mutex::default(),
                taken: false,
            }
        }

        fn where_the_name_is_taken() -> Self {
            Self {
                teams: Mutex::new(Vec::new()),
                written: Mutex::default(),
                taken: true,
            }
        }

        fn written(&self) -> Written {
            self.written.lock().expect("the lock").clone()
        }

        fn holds(&self, team: TeamId) -> bool {
            self.teams
                .lock()
                .expect("the lock")
                .iter()
                .any(|one| one.id == team)
        }
    }

    impl Squads for &Held {
        async fn of(&self, _org: TenantId) -> Result<Vec<Team>, ReadError> {
            Ok(self.teams.lock().expect("the lock").clone())
        }

        async fn create(&self, _org: TenantId, name: &TeamName) -> Result<Named, ReadError> {
            if self.taken {
                return Ok(Named::Taken);
            }
            self.written
                .lock()
                .expect("the lock")
                .created
                .push(name.as_str().to_owned());
            self.teams.lock().expect("the lock").push(Team {
                id: a_team(99),
                name: name.as_str().into(),
                created_at_unix_nanos: FOUNDED,
                members: Vec::new(),
                projects: Vec::new(),
            });
            Ok(Named::Free)
        }

        async fn rename(
            &self,
            _org: TenantId,
            team: TeamId,
            name: &TeamName,
        ) -> Result<Named, ReadError> {
            if self.taken {
                return Ok(Named::Taken);
            }
            self.written
                .lock()
                .expect("the lock")
                .renamed
                .push((team, name.as_str().to_owned()));
            Ok(Named::Free)
        }

        async fn disband(&self, _org: TenantId, team: TeamId) -> Result<bool, ReadError> {
            self.written.lock().expect("the lock").disbanded.push(team);
            Ok(self.holds(team))
        }

        async fn seat(
            &self,
            _org: TenantId,
            team: TeamId,
            members: &[AccountId],
        ) -> Result<bool, ReadError> {
            self.written
                .lock()
                .expect("the lock")
                .seated
                .push((team, members.to_vec()));
            Ok(self.holds(team))
        }

        async fn own(
            &self,
            _org: TenantId,
            team: TeamId,
            projects: &[ProjectId],
        ) -> Result<bool, ReadError> {
            self.written
                .lock()
                .expect("the lock")
                .owned
                .push((team, projects.to_vec()));
            Ok(self.holds(team))
        }
    }

    fn backend() -> Team {
        Team {
            id: a_team(BACKEND),
            name: "Backend".into(),
            created_at_unix_nanos: FOUNDED,
            members: vec![an_account(9)],
            projects: vec![Owned {
                id: a_project(7),
                name: "storefront".into(),
            }],
        }
    }

    fn squad(role: OrgRole, held: &Held) -> Squad<OneSession, MemberOfMine, &Held> {
        Squad::new(Whoami::new(OneSession, MemberOfMine(role)), held)
    }

    fn token() -> SessionToken {
        SessionToken::from_plaintext(TOKEN)
    }

    #[tokio::test]
    async fn a_member_reads_the_teams_of_its_own_organisation() {
        let held = Held::of(vec![backend()]);

        let teams = squad(OrgRole::Member, &held)
            .teams(&token(), an_org(MINE))
            .await
            .expect("a member may read");

        assert_eq!(teams, vec![backend()]);
    }

    #[tokio::test]
    async fn the_teams_of_another_organisation_are_refused() {
        let held = Held::of(vec![backend()]);

        let refused = squad(OrgRole::Owner, &held)
            .teams(&token(), an_org(THEIRS))
            .await;

        assert_eq!(refused, Err(TeamError::NotYours));
    }

    #[tokio::test]
    async fn a_token_no_session_carries_reads_nothing() {
        let held = Held::of(vec![backend()]);

        let refused = squad(OrgRole::Owner, &held)
            .teams(&SessionToken::from_plaintext("ct_ss_nope"), an_org(MINE))
            .await;

        assert_eq!(refused, Err(TeamError::NoSession));
    }

    #[tokio::test]
    async fn an_administrator_creates_a_team_and_is_answered_with_them_all() {
        let held = Held::of(vec![backend()]);

        let teams = squad(OrgRole::Admin, &held)
            .create(&token(), an_org(MINE), &a_name("Payments"))
            .await
            .expect("an administrator may create a team");

        assert_eq!(held.written().created, vec!["Payments".to_owned()]);
        assert_eq!(teams.len(), 2, "the answer carries the whole screen");
    }

    #[tokio::test]
    async fn a_member_arranges_nothing() {
        let held = Held::of(vec![backend()]);

        let refused = squad(OrgRole::Member, &held)
            .create(&token(), an_org(MINE), &a_name("Payments"))
            .await;

        assert_eq!(refused, Err(TeamError::NotYoursToChange));
        assert_eq!(held.written(), Written::default());
    }

    #[tokio::test]
    async fn a_name_the_organisation_already_holds_is_refused() {
        let held = Held::where_the_name_is_taken();

        let refused = squad(OrgRole::Admin, &held)
            .create(&token(), an_org(MINE), &a_name("Backend"))
            .await;

        assert_eq!(refused, Err(TeamError::NameTaken));
        assert_eq!(held.written(), Written::default());
    }

    #[tokio::test]
    async fn renaming_a_team_onto_a_name_already_held_is_refused_too() {
        let held = Held::where_the_name_is_taken();

        let refused = squad(OrgRole::Admin, &held)
            .rename(&token(), an_org(MINE), a_team(BACKEND), &a_name("Payments"))
            .await;

        assert_eq!(refused, Err(TeamError::NameTaken));
    }

    #[tokio::test]
    async fn an_administrator_disbands_a_team() {
        let held = Held::of(vec![backend()]);

        squad(OrgRole::Admin, &held)
            .disband(&token(), an_org(MINE), a_team(BACKEND))
            .await
            .expect("an administrator may disband a team");

        assert_eq!(held.written().disbanded, vec![a_team(BACKEND)]);
    }

    #[tokio::test]
    async fn a_team_this_organisation_does_not_hold_is_not_found() {
        let held = Held::of(vec![backend()]);

        let refused = squad(OrgRole::Owner, &held)
            .disband(&token(), an_org(MINE), a_team(404))
            .await;

        assert_eq!(refused, Err(TeamError::NoSuchTeam));
    }

    #[tokio::test]
    async fn who_is_on_a_team_is_written_whole() {
        let held = Held::of(vec![backend()]);
        let seated = [an_account(9), an_account(10)];

        squad(OrgRole::Admin, &held)
            .seat(&token(), an_org(MINE), a_team(BACKEND), &seated)
            .await
            .expect("an administrator may seat a team");

        assert_eq!(
            held.written().seated,
            vec![(a_team(BACKEND), seated.to_vec())]
        );
    }

    #[tokio::test]
    async fn seating_nobody_is_a_team_with_nobody_on_it_rather_than_a_refusal() {
        let held = Held::of(vec![backend()]);

        squad(OrgRole::Admin, &held)
            .seat(&token(), an_org(MINE), a_team(BACKEND), &[])
            .await
            .expect("a team may hold nobody");

        assert_eq!(held.written().seated, vec![(a_team(BACKEND), Vec::new())]);
    }

    #[tokio::test]
    async fn what_a_team_owns_is_written_whole() {
        let held = Held::of(vec![backend()]);
        let owned = [a_project(7), a_project(8)];

        squad(OrgRole::Admin, &held)
            .own(&token(), an_org(MINE), a_team(BACKEND), &owned)
            .await
            .expect("an administrator may hand a team its projects");

        assert_eq!(
            held.written().owned,
            vec![(a_team(BACKEND), owned.to_vec())]
        );
    }

    #[tokio::test]
    async fn arranging_a_team_of_another_organisation_says_no_such_thing() {
        let held = Held::of(vec![backend()]);

        let refused = squad(OrgRole::Owner, &held)
            .seat(&token(), an_org(THEIRS), a_team(BACKEND), &[])
            .await;

        assert_eq!(refused, Err(TeamError::NotYours));
        assert_eq!(held.written(), Written::default());
    }

    #[test]
    fn a_name_is_kept_without_the_space_around_it() {
        assert_eq!(a_name("  Backend \n").as_str(), "Backend");
    }

    #[test]
    fn a_name_that_is_only_space_is_as_blank_as_an_empty_one() {
        assert_eq!(TeamName::try_from(""), Err(BadTeamName::Blank));
        assert_eq!(TeamName::try_from("   \t\n"), Err(BadTeamName::Blank));
    }

    #[test]
    fn a_name_past_the_ceiling_is_refused() {
        let long = "a".repeat(MAX_NAME_BYTES + 1);

        assert_eq!(
            TeamName::try_from(long.as_str()),
            Err(BadTeamName::TooLong(MAX_NAME_BYTES))
        );
        assert!(TeamName::try_from("a".repeat(MAX_NAME_BYTES).as_str()).is_ok());
    }

    mod proptests {
        use proptest::prelude::*;

        use super::super::MAX_NAME_BYTES;
        use super::{BadTeamName, TeamName};

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

                let name = TeamName::try_from(padded.as_str()).expect("a name within the ceiling");

                prop_assert_eq!(name.as_str(), inner.as_str());
            }

            #[test]
            fn a_name_one_byte_past_the_ceiling_is_refused_whatever_it_contains(
                over in prop::collection::vec(0x21u8..=0x7eu8, MAX_NAME_BYTES + 1)
                    .prop_map(|bytes| String::from_utf8(bytes).expect("ascii bytes"))
            ) {
                prop_assert_eq!(
                    TeamName::try_from(over.as_str()),
                    Err(BadTeamName::TooLong(MAX_NAME_BYTES))
                );
            }
        }
    }
}
