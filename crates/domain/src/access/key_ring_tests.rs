use std::sync::Mutex;
use uuid::Uuid;

use super::entities::{ApiKey, IssuedKey};
use super::errors::KeyError;
use super::key_ring::KeyRing;
use super::ports::ApiKeys;
use super::value_objects::{ApiSecret, KeyId, KeyName, KeyScope, KeyScopes, ReadGrant};
use crate::audit::{Action, Actor, Target};
use crate::catalogue::{Assigned, Created, Project, ProjectId, ProjectName, Projects, Renamed};
use crate::clock::UnixSeconds;
use crate::identity::{AccountId, Agent, Email, Session, SessionError, SessionStore};
use crate::organisation::{Membership, Memberships, OrgRole, TeamId, Whoami, WhoamiError};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::TenantId;
use crate::testing::Notebook;

pub(crate) const TOKEN: &str = "ft_ss_live";
pub(crate) const MINE: u128 = 1;
pub(crate) const THEIRS: u128 = 2;
pub(crate) const OURS: u128 = 30;
pub(crate) const OTHER: u128 = 40;
const ACCOUNT: u128 = 9;

pub(crate) fn an_org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

pub(crate) fn a_project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

pub(crate) fn token() -> SessionToken {
    SessionToken::from_plaintext(TOKEN)
}

fn a_name(spelling: &str) -> KeyName {
    KeyName::try_from(spelling).expect("a name")
}

pub(crate) fn scopes(of: &[KeyScope]) -> KeyScopes {
    KeyScopes::new(of).expect("scopes")
}

pub(crate) struct OneSession;

impl SessionStore for OneSession {
    async fn issue(
        &self,
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Ok(token())
    }

    async fn resolve(&self, offered: &SessionToken) -> Result<Session, SessionError> {
        if offered.exposed() == TOKEN {
            Ok(Session {
                account: AccountId::new(Uuid::from_u128(ACCOUNT)),
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

pub(crate) struct MemberOfMine(pub(crate) OrgRole);

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: an_org(MINE),
            name: "Acme".into(),
            role: self.0,
        }])
    }
}

struct Held {
    org: TenantId,
    key: ApiKey,
    secret: String,
    revoked: bool,
}

#[derive(Default)]
pub(crate) struct Ring {
    held: Mutex<Vec<Held>>,
    minted: Mutex<u128>,
}

impl Ring {
    pub(crate) fn writes(&self) -> usize {
        self.held.lock().expect("the lock").len()
    }

    pub(crate) fn holding(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        scopes: KeyScopes,
    ) -> ApiSecret {
        let mut minted = self.minted.lock().expect("the lock");
        *minted += 1;
        let prefix = format!("ft_live_{:012x}", *minted);
        let secret = format!("{prefix}.{:064x}", *minted);
        self.held.lock().expect("the lock").push(Held {
            org,
            key: ApiKey {
                id: KeyId::new(Uuid::from_u128(1000 + *minted)),
                project,
                name: "held".into(),
                prefix: prefix.into(),
                scopes,
                created_at_unix_nanos: 1,
                created_by: None,
                last_used_at_unix_nanos: None,
            },
            secret: secret.clone(),
            revoked: false,
        });
        ApiSecret::from_plaintext(&secret)
    }
}

impl ApiKeys for &Ring {
    async fn list(&self, org: TenantId) -> Result<Vec<ApiKey>, StoreError> {
        Ok(self
            .held
            .lock()
            .expect("the lock")
            .iter()
            .filter(|held| held.org == org && !held.revoked)
            .map(|held| held.key.clone())
            .collect())
    }

    async fn issue(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        name: &KeyName,
        scopes: &KeyScopes,
        by: AccountId,
    ) -> Result<IssuedKey, StoreError> {
        let secret = self.holding(org, project, scopes.clone());
        let mut held = self.held.lock().expect("the lock");
        let last = held.last_mut().expect("just held");
        last.key.name = name.as_str().into();
        last.key.created_by = Some(by);
        Ok(IssuedKey {
            key: last.key.clone(),
            secret,
        })
    }

    async fn revoke(&self, org: TenantId, id: KeyId) -> Result<bool, StoreError> {
        let mut held = self.held.lock().expect("the lock");
        let found = held
            .iter_mut()
            .find(|held| held.org == org && held.key.id == id && !held.revoked);
        Ok(found.map(|held| held.revoked = true).is_some())
    }

    async fn grant_for(&self, secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(self
            .held
            .lock()
            .expect("the lock")
            .iter()
            .find(|held| {
                held.secret == secret.exposed()
                    && !held.revoked
                    && held.key.scopes.contains(KeyScope::Read)
            })
            .map(|held| ReadGrant {
                org: held.org,
                project: held.key.project,
            }))
    }
}

pub(crate) struct OneProject;

impl Projects for OneProject {
    async fn of(&self, tenant: TenantId) -> Result<Vec<Project>, StoreError> {
        Ok(vec![Project {
            id: a_project(OURS),
            org: tenant,
            name: "storefront".into(),
            created_at_unix_nanos: 1,
            team: None,
            keys: 0,
        }])
    }

    async fn create(&self, _org: TenantId, _name: &ProjectName) -> Result<Created, StoreError> {
        Ok(Created::Done)
    }

    async fn rename(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _name: &ProjectName,
    ) -> Result<Renamed, StoreError> {
        Ok(Renamed::Gone)
    }

    async fn forget(&self, _org: TenantId, _project: ProjectId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn assign(
        &self,
        _org: TenantId,
        _project: ProjectId,
        _team: Option<TeamId>,
    ) -> Result<Assigned, StoreError> {
        Ok(Assigned::Done)
    }
}

fn ring(
    role: OrgRole,
    keys: &Ring,
) -> KeyRing<OneSession, MemberOfMine, &Ring, OneProject, Notebook> {
    noting(role, keys, &Notebook::default())
}

fn noting<'a>(
    role: OrgRole,
    keys: &'a Ring,
    notebook: &Notebook,
) -> KeyRing<OneSession, MemberOfMine, &'a Ring, OneProject, Notebook> {
    KeyRing::new(
        Whoami::new(OneSession, MemberOfMine(role)),
        keys,
        OneProject,
        notebook.clone(),
    )
}

#[tokio::test]
async fn a_member_lists_the_live_keys_of_its_organisation() {
    let keys = Ring::default();
    keys.holding(an_org(MINE), None, KeyScopes::ingest());
    keys.holding(an_org(THEIRS), None, KeyScopes::ingest());

    let listed = ring(OrgRole::Member, &keys)
        .list(&token(), an_org(MINE))
        .await
        .expect("a member may list");

    assert_eq!(listed.len(), 1);
}

#[tokio::test]
async fn a_stranger_is_told_there_is_no_such_organisation() {
    let keys = Ring::default();

    let listed = ring(OrgRole::Owner, &keys)
        .list(&token(), an_org(THEIRS))
        .await;
    let issued = ring(OrgRole::Owner, &keys)
        .issue(
            &token(),
            an_org(THEIRS),
            None,
            &a_name("ci"),
            &KeyScopes::ingest(),
        )
        .await;

    assert_eq!(listed, Err(KeyError::NotYours));
    assert_eq!(issued.map(|issued| issued.key), Err(KeyError::NotYours));
    assert_eq!(keys.writes(), 0);
}

#[tokio::test]
async fn a_token_no_session_carries_lists_nothing() {
    let keys = Ring::default();

    let listed = ring(OrgRole::Owner, &keys)
        .list(&SessionToken::from_plaintext("ft_ss_nope"), an_org(MINE))
        .await;

    assert_eq!(listed, Err(KeyError::NoSession));
}

#[tokio::test]
async fn an_administrator_issues_a_key_whose_secret_comes_back_once() {
    let keys = Ring::default();

    let issued = ring(OrgRole::Admin, &keys)
        .issue(
            &token(),
            an_org(MINE),
            Some(a_project(OURS)),
            &a_name(" edge collector "),
            &scopes(&[KeyScope::Read]),
        )
        .await
        .expect("an administrator may issue");
    let listed = ring(OrgRole::Member, &keys)
        .list(&token(), an_org(MINE))
        .await
        .expect("a member may list");

    let (prefix, _) = issued.secret.split().expect("a well formed secret");
    assert_eq!(prefix, &*issued.key.prefix);
    assert_eq!(&*issued.key.name, "edge collector");
    assert_eq!(issued.key.project, Some(a_project(OURS)));
    assert_eq!(
        issued.key.created_by,
        Some(AccountId::new(Uuid::from_u128(ACCOUNT)))
    );
    assert_eq!(listed, vec![issued.key]);
}

#[tokio::test]
async fn a_member_may_neither_issue_nor_revoke_and_nothing_is_written() {
    let keys = Ring::default();
    keys.holding(an_org(MINE), None, KeyScopes::ingest());
    let held = ring(OrgRole::Admin, &keys)
        .list(&token(), an_org(MINE))
        .await
        .expect("a list")
        .remove(0);

    let issued = ring(OrgRole::Member, &keys)
        .issue(
            &token(),
            an_org(MINE),
            None,
            &a_name("ci"),
            &KeyScopes::ingest(),
        )
        .await;
    let revoked = ring(OrgRole::Member, &keys)
        .revoke(&token(), an_org(MINE), held.id)
        .await;

    assert_eq!(
        issued.map(|issued| issued.key),
        Err(KeyError::NotYoursToChange)
    );
    assert_eq!(revoked, Err(KeyError::NotYoursToChange));
    assert_eq!(keys.writes(), 1);
    assert_eq!(
        ring(OrgRole::Member, &keys)
            .list(&token(), an_org(MINE))
            .await
            .map(|listed| listed.len()),
        Ok(1)
    );
}

#[tokio::test]
async fn an_owner_revokes_a_key_and_it_grants_nothing_at_once() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, scopes(&[KeyScope::Read]));
    let held = ring(OrgRole::Owner, &keys)
        .list(&token(), an_org(MINE))
        .await
        .expect("a list")
        .remove(0);
    assert!((&keys).grant_for(&secret).await.expect("a store").is_some());

    ring(OrgRole::Owner, &keys)
        .revoke(&token(), an_org(MINE), held.id)
        .await
        .expect("an owner may revoke");

    assert_eq!((&keys).grant_for(&secret).await, Ok(None));
    assert_eq!(
        ring(OrgRole::Owner, &keys)
            .list(&token(), an_org(MINE))
            .await,
        Ok(Vec::new())
    );
}

#[tokio::test]
async fn revoking_a_key_nobody_holds_says_so() {
    let keys = Ring::default();

    let refused = ring(OrgRole::Owner, &keys)
        .revoke(&token(), an_org(MINE), KeyId::new(Uuid::from_u128(77)))
        .await;

    assert_eq!(refused, Err(KeyError::NoSuchKey));
}

#[tokio::test]
async fn a_key_is_only_bound_to_a_project_of_its_own_organisation() {
    let keys = Ring::default();

    let refused = ring(OrgRole::Owner, &keys)
        .issue(
            &token(),
            an_org(MINE),
            Some(a_project(OTHER)),
            &a_name("ci"),
            &KeyScopes::ingest(),
        )
        .await;

    assert_eq!(
        refused.map(|issued| issued.key),
        Err(KeyError::NoSuchProject)
    );
    assert_eq!(keys.writes(), 0);
}

#[tokio::test]
async fn issuing_and_revoking_a_key_records_each_once_and_never_its_secret() {
    let keys = Ring::default();
    let notebook = Notebook::default();
    let ring = noting(OrgRole::Admin, &keys, &notebook);

    let issued = ring
        .issue(
            &token(),
            an_org(MINE),
            None,
            &KeyName::try_from("ci").expect("a name"),
            &KeyScopes::new(&[KeyScope::Ingest, KeyScope::Read]).expect("scopes"),
        )
        .await
        .expect("an administrator issues a key");
    ring.revoke(&token(), an_org(MINE), issued.key.id)
        .await
        .expect("an administrator revokes a key");

    let entries = notebook.entries();
    let actions: Vec<Action> = entries.iter().map(|entry| entry.action).collect();
    assert_eq!(actions, vec![Action::KeyIssued, Action::KeyRevoked]);
    assert!(
        entries
            .iter()
            .all(|entry| entry.target == Target::key(issued.key.id))
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.actor == Actor::Account(AccountId::new(Uuid::from_u128(ACCOUNT))))
    );
    assert_eq!(entries[0].detail.get("name"), Some(Some("ci")));
    assert_eq!(entries[0].detail.get("scopes"), Some(Some("ingest,read")));
    assert_eq!(entries[0].detail.get("project"), Some(None));
    let (_, secret) = issued.secret.split().expect("a well formed secret");
    assert!(
        entries
            .iter()
            .all(|entry| entry.detail.pairs().iter().all(|(_, value)| {
                value.as_deref().is_none_or(|value| {
                    !value.contains(secret) && !value.contains(issued.secret.exposed())
                })
            }))
    );
}

#[tokio::test]
async fn a_refused_key_change_records_nothing() {
    let keys = Ring::default();
    let notebook = Notebook::default();

    let refused = noting(OrgRole::Member, &keys, &notebook)
        .issue(
            &token(),
            an_org(MINE),
            None,
            &KeyName::try_from("ci").expect("a name"),
            &KeyScopes::ingest(),
        )
        .await;

    assert_eq!(refused.err(), Some(KeyError::NotYoursToChange));
    assert!(notebook.entries().is_empty());
}
