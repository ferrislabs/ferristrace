use ferristrace_application::registry::{RepoFor, Settle, UnitOfWork, domain};
use ferristrace_domain::{
    ControlPlane, Email, FoundError, Founded, Founding, Membership, Memberships, OrgName, OrgRole,
    SessionError, SessionStore, SessionSwitch, SessionToken, Switching, TenantId,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;

async fn migrated(schema: &str) -> Option<PgPool> {
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

async fn an_account(pool: &PgPool, email: &str) -> ferristrace_domain::AccountId {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await
        .expect("an account");
    ferristrace_domain::AccountId::new(id)
}

async fn an_org_of(pg: &Postgres<FixedClock>, name: &str, owner: &str) -> TenantId {
    let owner = Email::try_from(owner).expect("an address");
    let name = OrgName::try_from(name).expect("a name");
    backend::control_plane_of(pg)
        .found(&name, &owner)
        .await
        .expect("an organisation")
}

async fn active_org(pool: &PgPool, token: &SessionToken) -> Option<Uuid> {
    sqlx::query("SELECT org_id FROM sessions WHERE hash = $1")
        .bind(token.hash().as_bytes().as_slice())
        .fetch_one(pool)
        .await
        .expect("the session row")
        .get("org_id")
}

async fn found(
    pg: &Postgres<FixedClock>,
    token: &SessionToken,
    name: &str,
) -> Result<Founded, FoundError> {
    let name = OrgName::try_from(name).expect("a name");
    UnitOfWork::transaction(
        pg,
        || FoundError::Unreachable,
        Settle::OnSuccess,
        |tx| async move {
            let sessions = <Postgres<FixedClock> as RepoFor<domain::Sessions>>::build(pg, &tx);
            let control_plane =
                <Postgres<FixedClock> as RepoFor<domain::ControlPlane>>::build(pg, &tx);
            let audit = <Postgres<FixedClock> as RepoFor<domain::Audit>>::build(pg, &tx);
            Founding::new(sessions, control_plane, audit)
                .found(token, &name)
                .await
        },
    )
    .await
}

#[tokio::test]
async fn switching_moves_only_the_current_session_to_the_organisation() {
    let Some(pool) = migrated("switch_current_only").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(NOW));
    let account = an_account(&pool, "person@example.com").await;
    let first = an_org_of(&pg, "First", "person@example.com").await;
    let second = an_org_of(&pg, "Second", "person@example.com").await;
    let sessions = backend::sessions(&pg);
    let here = sessions
        .issue(account, Some(first), None)
        .await
        .expect("a session");
    let elsewhere = sessions
        .issue(account, Some(first), None)
        .await
        .expect("another session");

    sessions.switch(&here, second).await.expect("it switches");

    assert_eq!(active_org(&pool, &here).await, Some(second.as_uuid()));
    assert_eq!(active_org(&pool, &elsewhere).await, Some(first.as_uuid()));
    assert_eq!(
        sessions.resolve(&here).await.expect("it resolves").org,
        Some(second)
    );
}

#[tokio::test]
async fn a_revoked_session_cannot_be_switched() {
    let Some(pool) = migrated("switch_revoked").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(NOW));
    let account = an_account(&pool, "person@example.com").await;
    let org = an_org_of(&pg, "First", "person@example.com").await;
    let sessions = backend::sessions(&pg);
    let token = sessions
        .issue(account, None, None)
        .await
        .expect("a session");
    sessions.revoke(&token).await.expect("it revokes");

    assert_eq!(
        sessions.switch(&token, org).await,
        Err(SessionError::Unknown)
    );
}

#[tokio::test]
async fn switching_is_refused_for_an_organisation_the_account_is_not_in() {
    let Some(pool) = migrated("switch_not_yours").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(NOW));
    let account = an_account(&pool, "person@example.com").await;
    an_account(&pool, "other@example.com").await;
    let mine = an_org_of(&pg, "Mine", "person@example.com").await;
    let theirs = an_org_of(&pg, "Theirs", "other@example.com").await;
    let sessions = backend::sessions(&pg);
    let token = sessions
        .issue(account, Some(mine), None)
        .await
        .expect("a session");

    let refused = Switching::new(sessions, backend::memberships(&pg))
        .to(&token, theirs)
        .await;

    assert_eq!(refused, Err(ferristrace_domain::SwitchError::NotYours));
    assert_eq!(active_org(&pool, &token).await, Some(mine.as_uuid()));
}

#[tokio::test]
async fn founding_from_a_session_makes_an_owned_org_with_a_default_project_and_switches_to_it() {
    let Some(pool) = migrated("switch_founding").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(NOW));
    let account = an_account(&pool, "person@example.com").await;
    let token = backend::sessions(&pg)
        .issue(account, None, None)
        .await
        .expect("a session");

    let founded = found(&pg, &token, "Acme").await.expect("a founded org");

    assert_eq!(founded.project_name.as_str(), "default");
    assert_eq!(active_org(&pool, &token).await, Some(founded.org.as_uuid()));
    let memberships: Vec<Membership> = backend::memberships(&pg)
        .of(account)
        .await
        .expect("memberships");
    assert_eq!(memberships.len(), 1);
    assert_eq!(memberships[0].org, founded.org);
    assert_eq!(&*memberships[0].name, "Acme");
    assert_eq!(memberships[0].role, OrgRole::Owner);
    let project = sqlx::query("SELECT id, name FROM projects WHERE org_id = $1")
        .bind(founded.org.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("the default project");
    assert_eq!(project.get::<Uuid, _>("id"), founded.project.as_uuid());
    assert_eq!(project.get::<String, _>("name"), "default");
    let entry = sqlx::query("SELECT actor_kind, action FROM audit_entries WHERE org_id = $1")
        .bind(founded.org.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("the founding is audited");
    assert_eq!(entry.get::<String, _>("actor_kind"), "account");
    assert_eq!(entry.get::<String, _>("action"), "org.founded");
}

#[tokio::test]
async fn a_founding_by_a_dead_session_leaves_no_organisation() {
    let Some(pool) = migrated("switch_founding_dead").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(NOW));
    an_account(&pool, "person@example.com").await;

    let refused = found(&pg, &SessionToken::from_plaintext("ft_ss_dead"), "Acme").await;

    assert_eq!(refused, Err(FoundError::NoSession));
    let orgs: i64 = sqlx::query("SELECT count(*) AS n FROM orgs")
        .fetch_one(&pool)
        .await
        .expect("a count")
        .get("n");
    assert_eq!(orgs, 0);
}
