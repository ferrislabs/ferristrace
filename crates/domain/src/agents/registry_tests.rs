use std::sync::Mutex;
use uuid::Uuid;

use super::entities::{Agent, AgentEnrolment};
use super::errors::AgentError;
use super::ports::Agents;
use super::registry::AgentRegistry;
use super::value_objects::{AgentId, AgentName, AgentScope, AgentScopes, ClientId};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OURS, OneProject, OneSession, THEIRS, a_project, an_org, token,
};
use crate::access::{ApiKeys, ApiSecret, ReadGrant};
use crate::audit::{Action, Actor, AuditEntry, Target, TargetKind};
use crate::identity::AccountId;
use crate::organisation::{OrgRole, Whoami};
use crate::secret::SessionToken;
use crate::store::StoreError;
use crate::telemetry::TenantId;
use crate::testing::Notebook;

#[derive(Default)]
pub(crate) struct Book {
    agents: Mutex<Vec<Agent>>,
    used: Mutex<Vec<(AgentId, AuditEntry)>>,
    minted: Mutex<u128>,
}

impl Book {
    pub(crate) fn enrol(
        &self,
        org: TenantId,
        client_id: &str,
        scopes: &[AgentScope],
        project: Option<crate::catalogue::ProjectId>,
    ) -> AgentId {
        let mut minted = self.minted.lock().expect("the lock");
        *minted += 1;
        let id = AgentId::new(Uuid::from_u128(5000 + *minted));
        self.agents.lock().expect("the lock").push(Agent {
            id,
            org,
            name: "triage".into(),
            client_id: client_id.into(),
            scopes: AgentScopes::new(scopes).expect("scopes"),
            project,
            created_by: None,
            created_at_unix_nanos: 1,
            last_used_at_unix_nanos: None,
            revoked_at_unix_nanos: None,
        });
        id
    }

    pub(crate) fn revoke_now(&self, id: AgentId) {
        for agent in self.agents.lock().expect("the lock").iter_mut() {
            if agent.id == id {
                agent.revoked_at_unix_nanos = Some(2);
            }
        }
    }

    pub(crate) fn uses(&self) -> Vec<(AgentId, AuditEntry)> {
        self.used.lock().expect("the lock").clone()
    }

    fn count(&self) -> usize {
        self.agents.lock().expect("the lock").len()
    }
}

impl Agents for &Book {
    async fn list(&self, org: TenantId) -> Result<Vec<Agent>, StoreError> {
        Ok(self
            .agents
            .lock()
            .expect("the lock")
            .iter()
            .filter(|agent| agent.org == org)
            .cloned()
            .collect())
    }

    async fn register(
        &self,
        org: TenantId,
        enrolment: &AgentEnrolment,
        by: AccountId,
    ) -> Result<Option<Agent>, StoreError> {
        let taken = self.agents.lock().expect("the lock").iter().any(|agent| {
            agent.org == org
                && !agent.is_revoked()
                && &*agent.client_id == enrolment.client_id.as_str()
        });
        if taken {
            return Ok(None);
        }
        let id = self.enrol(
            org,
            enrolment.client_id.as_str(),
            enrolment.scopes.as_slice(),
            enrolment.project,
        );
        let mut agents = self.agents.lock().expect("the lock");
        let agent = agents
            .iter_mut()
            .find(|agent| agent.id == id)
            .expect("just enrolled");
        agent.name = enrolment.name.as_str().into();
        agent.created_by = Some(by);
        Ok(Some(agent.clone()))
    }

    async fn revoke(&self, org: TenantId, id: AgentId) -> Result<Option<Agent>, StoreError> {
        let mut agents = self.agents.lock().expect("the lock");
        let found = agents
            .iter_mut()
            .find(|agent| agent.org == org && agent.id == id && !agent.is_revoked());
        Ok(found.map(|agent| {
            agent.revoked_at_unix_nanos = Some(2);
            agent.clone()
        }))
    }
}

impl ApiKeys for &Book {
    async fn list(&self, _org: TenantId) -> Result<Vec<crate::access::ApiKey>, StoreError> {
        Ok(Vec::new())
    }

    async fn issue(
        &self,
        _org: TenantId,
        _project: Option<crate::catalogue::ProjectId>,
        _name: &crate::access::KeyName,
        _scopes: &crate::access::KeyScopes,
        _by: AccountId,
        _ttl: crate::access::KeyTtl,
    ) -> Result<crate::access::IssuedKey, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn revoke(&self, _org: TenantId, _id: crate::access::KeyId) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn grant_for(&self, _secret: &ApiSecret) -> Result<Option<ReadGrant>, StoreError> {
        Ok(None)
    }

    async fn agent_standing(
        &self,
        org: TenantId,
        client_id: &str,
    ) -> Result<Option<Agent>, StoreError> {
        let agents = self.agents.lock().expect("the lock");
        let mut found: Vec<&Agent> = agents
            .iter()
            .filter(|agent| agent.org == org && &*agent.client_id == client_id)
            .collect();
        found.sort_by_key(|agent| agent.is_revoked());
        Ok(found.first().map(|agent| (*agent).clone()))
    }

    async fn agent_used(
        &self,
        _org: TenantId,
        agent: AgentId,
        entry: &AuditEntry,
    ) -> Result<(), StoreError> {
        self.used
            .lock()
            .expect("the lock")
            .push((agent, entry.clone()));
        Ok(())
    }
}

fn registry<'a>(
    role: OrgRole,
    book: &'a Book,
    notebook: &Notebook,
) -> AgentRegistry<OneSession, MemberOfMine, &'a Book, OneProject, Notebook> {
    AgentRegistry::new(
        Whoami::new(OneSession, MemberOfMine(role)),
        book,
        OneProject,
        notebook.clone(),
    )
}

fn enrolment(client_id: &str, project: Option<crate::catalogue::ProjectId>) -> AgentEnrolment {
    AgentEnrolment {
        name: AgentName::try_from("triage").expect("a name"),
        client_id: ClientId::try_from(client_id).expect("a client id"),
        scopes: AgentScopes::new(&[AgentScope::TelemetryRead]).expect("scopes"),
        project,
    }
}

#[tokio::test]
async fn an_administrator_registers_an_agent_and_the_register_is_audited() {
    let book = Book::default();
    let notebook = Notebook::default();

    let agent = registry(OrgRole::Admin, &book, &notebook)
        .register(
            &token(),
            an_org(MINE),
            &enrolment("triage-bot", Some(a_project(OURS))),
        )
        .await
        .expect("an administrator may register");

    assert_eq!(&*agent.client_id, "triage-bot");
    assert_eq!(agent.project, Some(a_project(OURS)));
    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::AgentRegistered);
    assert_eq!(entries[0].target, Target::agent(agent.id));
    assert_eq!(entries[0].target.kind, TargetKind::Agent);
    assert!(matches!(entries[0].actor, Actor::Account(_)));
    assert_eq!(entries[0].detail.get("client_id"), Some(Some("triage-bot")));
    assert_eq!(
        entries[0].detail.get("scopes"),
        Some(Some("telemetry:read"))
    );
}

#[tokio::test]
async fn only_an_administrator_registers_lists_or_revokes() {
    let book = Book::default();
    let id = book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);
    let notebook = Notebook::default();

    for role in [OrgRole::Member] {
        let registry = registry(role, &book, &notebook);
        assert_eq!(
            registry
                .register(&token(), an_org(MINE), &enrolment("other", None))
                .await,
            Err(AgentError::NotYoursToChange)
        );
        assert_eq!(
            registry.list(&token(), an_org(MINE)).await,
            Err(AgentError::NotYoursToChange)
        );
        assert_eq!(
            registry.revoke(&token(), an_org(MINE), id).await,
            Err(AgentError::NotYoursToChange)
        );
    }
    assert_eq!(book.count(), 1);
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn a_stranger_is_told_there_is_no_such_organisation() {
    let book = Book::default();
    let notebook = Notebook::default();
    let registry = registry(OrgRole::Owner, &book, &notebook);

    assert_eq!(
        registry.list(&token(), an_org(THEIRS)).await,
        Err(AgentError::NotYours)
    );
    assert_eq!(
        registry
            .register(&token(), an_org(THEIRS), &enrolment("bot", None))
            .await,
        Err(AgentError::NotYours)
    );
    assert_eq!(
        registry
            .revoke(&token(), an_org(THEIRS), AgentId::new(Uuid::from_u128(1)))
            .await,
        Err(AgentError::NotYours)
    );
    assert_eq!(
        registry
            .list(&SessionToken::for_subject("ft_ss_nope"), an_org(MINE))
            .await,
        Err(AgentError::NoSession)
    );
    assert_eq!(book.count(), 0);
}

#[tokio::test]
async fn a_client_id_is_registered_once_per_organisation_while_live() {
    let book = Book::default();
    let notebook = Notebook::default();
    let registry = registry(OrgRole::Admin, &book, &notebook);
    let first = registry
        .register(&token(), an_org(MINE), &enrolment("bot", None))
        .await
        .expect("a first registration");

    let twice = registry
        .register(&token(), an_org(MINE), &enrolment("bot", None))
        .await;
    registry
        .revoke(&token(), an_org(MINE), first.id)
        .await
        .expect("a revocation");
    let again = registry
        .register(&token(), an_org(MINE), &enrolment("bot", None))
        .await;

    assert_eq!(twice, Err(AgentError::AlreadyRegistered));
    assert!(again.is_ok());
}

#[tokio::test]
async fn a_project_of_another_organisation_is_refused() {
    let book = Book::default();
    let notebook = Notebook::default();

    let refused = registry(OrgRole::Admin, &book, &notebook)
        .register(
            &token(),
            an_org(MINE),
            &enrolment("bot", Some(a_project(999))),
        )
        .await;

    assert_eq!(refused, Err(AgentError::NoSuchProject));
    assert_eq!(book.count(), 0);
    assert!(notebook.entries().is_empty());
}

#[tokio::test]
async fn revoking_an_agent_is_audited_and_a_second_revocation_finds_nothing() {
    let book = Book::default();
    let id = book.enrol(an_org(MINE), "bot", &[AgentScope::TelemetryRead], None);
    let notebook = Notebook::default();
    let registry = registry(OrgRole::Admin, &book, &notebook);

    registry
        .revoke(&token(), an_org(MINE), id)
        .await
        .expect("a revocation");
    let second = registry.revoke(&token(), an_org(MINE), id).await;

    assert_eq!(second, Err(AgentError::NoSuchAgent));
    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::AgentRevoked);
    assert_eq!(entries[0].target, Target::agent(id));
}
