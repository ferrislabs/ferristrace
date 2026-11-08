use ferristrace_domain::{Orgs, TenantId};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
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

async fn an_account(pool: &PgPool, email: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await
        .expect("inserting an account");
    id
}

async fn an_org(pool: &PgPool, name: &str, owner: &str) -> TenantId {
    backend::memberships(&backend_of(pool))
        .found(name, owner)
        .await
        .expect("the organisation is founded")
}

async fn everything_an_org_owns(pool: &PgPool, org: TenantId, account: Uuid, label: &str) {
    sqlx::query("INSERT INTO projects (id, org_id, name) VALUES ($1, $2, $3)")
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(label)
        .execute(pool)
        .await
        .expect("a project");
    sqlx::query("INSERT INTO api_keys (id, org_id, prefix, hash) VALUES ($1, $2, $3, 'argon')")
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(format!("ft_live_{label}"))
        .execute(pool)
        .await
        .expect("an api key");
    sqlx::query("INSERT INTO teams (id, org_id, name) VALUES ($1, $2, $3)")
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(label)
        .execute(pool)
        .await
        .expect("a team");
    sqlx::query(
        "INSERT INTO audit_entries (id, org_id, actor_kind, actor_id, action, target_kind, at) \
         VALUES ($1, $2, 'account', $3, 'org.renamed', 'org', 0)",
    )
    .bind(Uuid::new_v4())
    .bind(org.as_uuid())
    .bind(account)
    .execute(pool)
    .await
    .expect("an audit entry");
    sqlx::query("UPDATE accounts SET active_org_id = $2 WHERE id = $1")
        .bind(account)
        .bind(org.as_uuid())
        .execute(pool)
        .await
        .expect("an active organisation");
}

async fn rows_of(pool: &PgPool, table: &str, org: TenantId) -> i64 {
    sqlx::query(AssertSqlSafe(format!(
        "SELECT count(*) AS held FROM {table} WHERE org_id = $1"
    )))
    .bind(org.as_uuid())
    .fetch_one(pool)
    .await
    .expect("counting the rows")
    .get("held")
}

const OWNED: [&str; 5] = [
    "projects",
    "api_keys",
    "teams",
    "audit_entries",
    "org_members",
];

#[tokio::test]
async fn an_organisation_is_deleted_and_reads_back_as_nothing() {
    let Some(pool) = control_plane("org_disband_gone").await else {
        return;
    };
    an_account(&pool, "owner@example.com").await;
    let org = an_org(&pool, "Acme", "owner@example.com").await;
    let pg = backend_of(&pool);

    let disbanded = backend::orgs(&pg).disband(org).await.expect("the delete");

    assert!(disbanded);
    assert!(
        backend::orgs(&pg)
            .only(org)
            .await
            .expect("the read")
            .is_none()
    );
}

#[tokio::test]
async fn disbanding_takes_everything_the_organisation_owned_with_it() {
    let Some(pool) = control_plane("org_disband_cascade").await else {
        return;
    };
    let owner = an_account(&pool, "owner@example.com").await;
    let other = an_account(&pool, "other@example.com").await;
    let org = an_org(&pool, "Acme", "owner@example.com").await;
    let kept = an_org(&pool, "Globex", "other@example.com").await;
    everything_an_org_owns(&pool, org, owner, "checkout").await;
    everything_an_org_owns(&pool, kept, other, "billing").await;

    let disbanded = backend::orgs(&backend_of(&pool))
        .disband(org)
        .await
        .expect("the delete");

    assert!(disbanded);
    for table in OWNED {
        assert_eq!(
            rows_of(&pool, table, org).await,
            0,
            "{table} still holds rows"
        );
        assert_ne!(
            rows_of(&pool, table, kept).await,
            0,
            "{table} lost a row of another organisation"
        );
    }
}

#[tokio::test]
async fn the_account_that_was_in_it_keeps_no_dangling_organisation() {
    let Some(pool) = control_plane("org_disband_active").await else {
        return;
    };
    let owner = an_account(&pool, "owner@example.com").await;
    let org = an_org(&pool, "Acme", "owner@example.com").await;
    everything_an_org_owns(&pool, org, owner, "checkout").await;

    backend::orgs(&backend_of(&pool))
        .disband(org)
        .await
        .expect("the delete");

    let active: Option<Uuid> =
        sqlx::query("SELECT active_org_id AS org FROM accounts WHERE id = $1")
            .bind(owner)
            .fetch_one(&pool)
            .await
            .expect("the account outlives its organisation")
            .get("org");
    assert_eq!(active, None);
}

#[tokio::test]
async fn disbanding_an_organisation_no_row_carries_says_so_rather_than_failing() {
    let Some(pool) = control_plane("org_disband_absent").await else {
        return;
    };

    let disbanded = backend::orgs(&backend_of(&pool))
        .disband(TenantId::new(Uuid::new_v4()))
        .await
        .expect("the delete is no error");

    assert!(!disbanded);
}
