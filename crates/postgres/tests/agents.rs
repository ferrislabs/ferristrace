use ferristrace_application::registry::{RepoFor, Settle, UnitOfWork, domain};
use ferristrace_domain::{
    AccountId, Action, Actor, AgentCall, AgentEnrolment, AgentId, AgentName, AgentScope,
    AgentScopes, Agents, ApiKeys, AuditEntry, AuditFilter, AuditTrail, ClientId, Detail, PageSize,
    ProjectId, ProjectName, Projects, ReadGrant, ReadRefusal, Reader, Readers, Target, TenantId,
    VerifiedToken, Whoami,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;

async fn control_plane(schema: &str) -> Option<PgPool> {
    let url = backend::url()?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connecting to postgres");
    sqlx::query(AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&pool)
    .await
    .expect("dropping a leftover schema");
    sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .expect("creating the test schema");
    pool.close().await;

    let owned = schema.to_owned();
    let scoped = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(move |connection, _| {
            let schema = owned.clone();
            Box::pin(async move {
                sqlx::query(AssertSqlSafe(format!("SET search_path TO {schema}")))
                    .execute(connection)
                    .await
                    .map(|_| ())
            })
        })
        .connect(&url)
        .await
        .expect("connecting to the test schema");
    CONTROL_PLANE
        .run(&scoped)
        .await
        .expect("the migrations apply");
    Some(scoped)
}

fn backend_of(pool: &PgPool) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(NOW))
}

async fn an_org(pool: &PgPool, owner: &str) -> (TenantId, AccountId) {
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'chosen')")
        .bind(account)
        .bind(owner)
        .execute(pool)
        .await
        .expect("inserting an account");
    let org = backend::memberships(&backend_of(pool))
        .found("Acme", owner)
        .await
        .expect("the organisation is founded");
    (org, AccountId::new(account))
}

async fn a_project(pool: &PgPool, org: TenantId, name: &str) -> ProjectId {
    let pg = backend_of(pool);
    let projects = backend::projects(&pg);
    projects
        .create(org, &ProjectName::try_from(name).expect("a slug"))
        .await
        .expect("the create");
    projects
        .of(org)
        .await
        .expect("the projects")
        .into_iter()
        .find(|project| &*project.name == name)
        .expect("the project is there")
        .id
}

fn enrolment(client_id: &str, scopes: &[AgentScope], project: Option<ProjectId>) -> AgentEnrolment {
    AgentEnrolment {
        name: AgentName::try_from("triage bot").expect("a name"),
        client_id: ClientId::try_from(client_id).expect("a client id"),
        scopes: AgentScopes::new(scopes).expect("scopes"),
        project,
    }
}

fn reading(org: TenantId, id: AgentId, jti: &str) -> AuditEntry {
    AuditEntry {
        org,
        actor: Actor::Agent(id),
        action: Action::AgentRead,
        target: Target::org(org),
        detail: Detail::default()
            .with("route", "/api/orgs/{org}/traces")
            .with("jti", jti),
    }
}

#[tokio::test]
async fn a_registered_agent_is_listed_with_its_scopes_project_and_creator() {
    let Some(pool) = control_plane("agents_register").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let storefront = a_project(&pool, org, "storefront").await;
    let agents = backend::agents(&pg);

    let registered = agents
        .register(
            org,
            &enrolment(
                "triage-bot",
                &[AgentScope::IncidentsComment, AgentScope::TelemetryRead],
                Some(storefront),
            ),
            owner,
        )
        .await
        .expect("the register")
        .expect("a fresh client id");
    let listed = agents.list(org).await.expect("the list");

    assert_eq!(listed, vec![registered.clone()]);
    assert_eq!(&*registered.name, "triage bot");
    assert_eq!(&*registered.client_id, "triage-bot");
    assert_eq!(
        registered.scopes.as_slice(),
        [AgentScope::TelemetryRead, AgentScope::IncidentsComment]
    );
    assert_eq!(registered.project, Some(storefront));
    assert_eq!(registered.created_by, Some(owner));
    assert_eq!(registered.created_at_unix_nanos, NOW * 1_000_000_000);
    assert_eq!(registered.last_used_at_unix_nanos, None);
    assert_eq!(registered.revoked_at_unix_nanos, None);
}

#[tokio::test]
async fn a_client_id_is_unique_per_organisation_among_live_agents() {
    let Some(pool) = control_plane("agents_unique").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let (other, other_owner) = {
        let account = Uuid::new_v4();
        sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'chosen')")
            .bind(account)
            .bind("other@example.com")
            .execute(&pool)
            .await
            .expect("inserting an account");
        let org = backend::memberships(&pg)
            .found("Globex", "other@example.com")
            .await
            .expect("another organisation");
        (org, AccountId::new(account))
    };
    let agents = backend::agents(&pg);
    let first = agents
        .register(
            org,
            &enrolment("bot", &[AgentScope::TelemetryRead], None),
            owner,
        )
        .await
        .expect("the register")
        .expect("a first");

    let twice = agents
        .register(
            org,
            &enrolment("bot", &[AgentScope::TelemetryRead], None),
            owner,
        )
        .await
        .expect("the register");
    let elsewhere = agents
        .register(
            other,
            &enrolment("bot", &[AgentScope::TelemetryRead], None),
            other_owner,
        )
        .await
        .expect("the register");
    let revoked = agents.revoke(org, first.id).await.expect("the revoke");
    let again = agents
        .register(
            org,
            &enrolment("bot", &[AgentScope::TelemetryRead], None),
            owner,
        )
        .await
        .expect("the register");

    assert_eq!(twice, None);
    assert!(elsewhere.is_some());
    assert!(revoked.is_some());
    assert!(again.is_some());
    assert_eq!(agents.list(org).await.expect("the list").len(), 2);
}

#[tokio::test]
async fn revoking_stamps_the_agent_once_and_only_in_its_organisation() {
    let Some(pool) = control_plane("agents_revoke").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let agents = backend::agents(&pg);
    let agent = agents
        .register(
            org,
            &enrolment("bot", &[AgentScope::TelemetryRead], None),
            owner,
        )
        .await
        .expect("the register")
        .expect("a first");

    let elsewhere = agents
        .revoke(TenantId::new(Uuid::new_v4()), agent.id)
        .await
        .expect("the revoke");
    let revoked = agents
        .revoke(org, agent.id)
        .await
        .expect("the revoke")
        .expect("a live agent");
    let twice = agents.revoke(org, agent.id).await.expect("the revoke");
    let listed = agents.list(org).await.expect("the list");

    assert_eq!(elsewhere, None);
    assert_eq!(revoked.revoked_at_unix_nanos, Some(NOW * 1_000_000_000));
    assert_eq!(twice, None);
    assert_eq!(listed, vec![revoked]);
}

#[tokio::test]
async fn the_database_refuses_a_scope_that_is_not_an_agent_scope() {
    let Some(pool) = control_plane("agents_scope_check").await else {
        return;
    };
    let (org, _owner) = an_org(&pool, "owner@example.com").await;

    for scopes in [
        vec!["admin"],
        vec!["telemetry:read", "read"],
        vec!["ingest"],
        vec![],
    ] {
        let refused = sqlx::query(
            "INSERT INTO agents (id, org_id, name, client_id, scopes, created_at) \
             VALUES ($1, $2, 'bot', 'bot', $3, 1)",
        )
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(scopes.clone())
        .execute(&pool)
        .await;

        assert!(refused.is_err(), "{scopes:?}");
    }
}

#[tokio::test]
async fn standing_prefers_the_live_agent_and_never_crosses_organisations() {
    let Some(pool) = control_plane("agents_standing").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let agents = backend::agents(&pg);
    let keys = backend::api_keys(&pg);
    let old = agents
        .register(
            org,
            &enrolment("bot", &[AgentScope::TelemetryRead], None),
            owner,
        )
        .await
        .expect("the register")
        .expect("a first");
    assert_eq!(
        keys.agent_standing(org, "bot").await.expect("a lookup"),
        Some(old.clone())
    );
    agents.revoke(org, old.id).await.expect("the revoke");
    let revoked_only = keys.agent_standing(org, "bot").await.expect("a lookup");
    let fresh = agents
        .register(
            org,
            &enrolment("bot", &[AgentScope::TelemetryRead], None),
            owner,
        )
        .await
        .expect("the register")
        .expect("a second");

    let live = keys.agent_standing(org, "bot").await.expect("a lookup");
    let elsewhere = keys
        .agent_standing(TenantId::new(Uuid::new_v4()), "bot")
        .await
        .expect("a lookup");
    let unknown = keys.agent_standing(org, "nobody").await.expect("a lookup");

    assert!(revoked_only.expect("kept").is_revoked());
    assert_eq!(live, Some(fresh));
    assert_eq!(elsewhere, None);
    assert_eq!(unknown, None);
}

#[tokio::test]
async fn an_agent_read_notes_the_use_and_leaves_an_audit_entry_labelled_with_the_name() {
    let Some(pool) = control_plane("agents_used").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let agents = backend::agents(&pg);
    let agent = agents
        .register(
            org,
            &enrolment("bot", &[AgentScope::TelemetryRead], None),
            owner,
        )
        .await
        .expect("the register")
        .expect("a first");

    backend::api_keys(&pg)
        .agent_used(org, agent.id, &reading(org, agent.id, "jti-77"))
        .await
        .expect("the use is noted");

    let listed = agents.list(org).await.expect("the list");
    assert_eq!(listed[0].last_used_at_unix_nanos, Some(NOW * 1_000_000_000));
    let page = backend::audit(&pg)
        .list(org, &AuditFilter::default(), PageSize::default())
        .await
        .expect("the log");
    assert_eq!(page.entries.len(), 1);
    let logged = &page.entries[0];
    assert_eq!(logged.entry.actor, Actor::Agent(agent.id));
    assert_eq!(logged.entry.action, Action::AgentRead);
    assert_eq!(logged.entry.detail.get("jti"), Some(Some("jti-77")));
    assert_eq!(
        logged.entry.detail.get("route"),
        Some(Some("/api/orgs/{org}/traces"))
    );
    assert_eq!(logged.label.as_deref(), Some("triage bot"));
    let by_agent = backend::audit(&pg)
        .list(
            org,
            &AuditFilter {
                actor: Some(Actor::Agent(agent.id)),
                ..AuditFilter::default()
            },
            PageSize::default(),
        )
        .await
        .expect("the log, filtered on the agent");
    assert_eq!(by_agent.entries.len(), 1);
}

#[tokio::test]
async fn the_database_refuses_an_agent_entry_without_an_agent() {
    let Some(pool) = control_plane("agents_anonymous").await else {
        return;
    };
    let (org, _owner) = an_org(&pool, "owner@example.com").await;

    let refused = sqlx::query(
        "INSERT INTO audit_entries (id, org_id, actor_kind, actor_id, action, target_kind, at) \
         VALUES ($1, $2, 'agent', NULL, 'agent.read', 'org', 1)",
    )
    .bind(Uuid::new_v4())
    .bind(org.as_uuid())
    .execute(&pool)
    .await;

    assert!(refused.is_err());
}

async fn grant_through_the_repositories(
    pg: &Postgres<FixedClock>,
    reader: &Reader,
    org: TenantId,
) -> Result<ReadGrant, ReadRefusal> {
    UnitOfWork::transaction(
        pg,
        || ReadRefusal::StoreUnreachable,
        Settle::OnSuccess,
        |tx| {
            let sessions = <_ as RepoFor<domain::Sessions>>::build(pg, &tx);
            let memberships = <_ as RepoFor<domain::Memberships>>::build(pg, &tx);
            let keys = <_ as RepoFor<domain::ApiKeys>>::build(pg, &tx);
            async move {
                Readers::new(Whoami::new(sessions, memberships), keys)
                    .grant(reader, org)
                    .await
            }
        },
    )
    .await
}

fn calling(client_id: &str, scopes: &[&str], jti: &str) -> Reader {
    Reader::Agent(AgentCall {
        token: VerifiedToken {
            client_id: client_id.into(),
            scopes: scopes.iter().map(|scope| (*scope).into()).collect(),
            jti: jti.into(),
            expires_at_unix_secs: NOW + 600,
        },
        route: "/api/orgs/{org}/traces".into(),
    })
}

#[tokio::test]
async fn a_read_through_the_real_repositories_leaves_exactly_one_entry_and_a_refusal_none() {
    let Some(pool) = control_plane("agents_grant").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let storefront = a_project(&pool, org, "storefront").await;
    let agent = backend::agents(&pg)
        .register(
            org,
            &enrolment("bot", &[AgentScope::TelemetryRead], Some(storefront)),
            owner,
        )
        .await
        .expect("the register")
        .expect("a first");

    let granted =
        grant_through_the_repositories(&pg, &calling("bot", &["telemetry:read"], "jti-1"), org)
            .await;
    let no_scope =
        grant_through_the_repositories(&pg, &calling("bot", &["openid"], "jti-2"), org).await;
    let stranger = grant_through_the_repositories(
        &pg,
        &calling("bot", &["telemetry:read"], "jti-3"),
        TenantId::new(Uuid::new_v4()),
    )
    .await;
    backend::agents(&pg)
        .revoke(org, agent.id)
        .await
        .expect("the revoke");
    let revoked =
        grant_through_the_repositories(&pg, &calling("bot", &["telemetry:read"], "jti-4"), org)
            .await;

    assert_eq!(
        granted,
        Ok(ReadGrant {
            org,
            project: Some(storefront),
        })
    );
    assert_eq!(no_scope, Err(ReadRefusal::OutOfScope));
    assert_eq!(stranger, Err(ReadRefusal::Unknown));
    assert_eq!(revoked, Err(ReadRefusal::Unknown));
    let page = backend::audit(&pg)
        .list(org, &AuditFilter::default(), PageSize::default())
        .await
        .expect("the log");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].entry.detail.get("jti"), Some(Some("jti-1")));
}
