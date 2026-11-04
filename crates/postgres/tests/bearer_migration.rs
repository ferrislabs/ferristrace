use ferristrace_postgres::CONTROL_PLANE;
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

#[macro_use]
mod backend;

const BEFORE: i64 = 20_261_115_000_002;
const SELF_HOSTED: &str = "00000000-0000-0000-0000-000000000001";

async fn bare(schema: &str) -> Option<PgPool> {
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
    Some(
        PgPoolOptions::new()
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
            .expect("connecting to the test schema"),
    )
}

async fn account(pool: &PgPool, email: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await
        .expect("an account");
    id
}

async fn org(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO orgs (id, name, plan_id) VALUES ($1, $2, $3::uuid)")
        .bind(id)
        .bind(name)
        .bind(SELF_HOSTED)
        .execute(pool)
        .await
        .expect("an org");
    id
}

async fn session(pool: &PgPool, account: Uuid, org: Option<Uuid>, seen: i64, revoked: Option<i64>) {
    sqlx::query(
        "INSERT INTO sessions (hash, account_id, org_id, created_at, last_seen_at, expires_at, revoked_at) \
         VALUES ($1, $2, $3, 1, $4, 9, $5)",
    )
    .bind(Uuid::new_v4().as_bytes().as_slice())
    .bind(account)
    .bind(org)
    .bind(seen)
    .bind(revoked)
    .execute(pool)
    .await
    .expect("a session");
}

async fn tables(pool: &PgPool) -> Vec<String> {
    sqlx::query(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = current_schema() AND table_name IN ('sessions', 'oidc_flows') \
         ORDER BY table_name",
    )
    .fetch_all(pool)
    .await
    .expect("the catalogue")
    .iter()
    .map(|row| row.get("table_name"))
    .collect()
}

async fn active(pool: &PgPool, account: Uuid) -> Option<Uuid> {
    sqlx::query("SELECT active_org_id FROM accounts WHERE id = $1")
        .bind(account)
        .fetch_one(pool)
        .await
        .expect("the account")
        .get("active_org_id")
}

#[tokio::test]
async fn the_latest_live_session_organisation_becomes_the_accounts_active_one() {
    let Some(pool) = bare("bearer_up").await else {
        return;
    };
    CONTROL_PLANE
        .run_to(BEFORE, &pool)
        .await
        .expect("the earlier migrations apply");
    let chooser = account(&pool, "chooser@example.com").await;
    let drifter = account(&pool, "drifter@example.com").await;
    let newcomer = account(&pool, "newcomer@example.com").await;
    let first = org(&pool, "First").await;
    let second = org(&pool, "Second").await;
    let third = org(&pool, "Third").await;
    session(&pool, chooser, Some(first), 10, None).await;
    session(&pool, chooser, Some(second), 20, None).await;
    session(&pool, chooser, Some(third), 30, Some(31)).await;
    session(&pool, drifter, None, 40, None).await;

    CONTROL_PLANE
        .run(&pool)
        .await
        .expect("the migration applies");

    assert_eq!(active(&pool, chooser).await, Some(second));
    assert_eq!(active(&pool, drifter).await, None);
    assert_eq!(active(&pool, newcomer).await, None);
    assert!(tables(&pool).await.is_empty());
}

#[tokio::test]
async fn going_back_recreates_both_tables_empty_and_drops_the_column() {
    let Some(pool) = bare("bearer_down").await else {
        return;
    };
    CONTROL_PLANE
        .run(&pool)
        .await
        .expect("the migrations apply");

    CONTROL_PLANE
        .undo(&pool, BEFORE)
        .await
        .expect("the migration undoes");

    assert_eq!(tables(&pool).await, vec!["oidc_flows", "sessions"]);
    let sessions: i64 = sqlx::query("SELECT count(*) AS n FROM sessions")
        .fetch_one(&pool)
        .await
        .expect("a count")
        .get("n");
    assert_eq!(sessions, 0);
    let column = sqlx::query(
        "SELECT 1 AS one FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'accounts' \
           AND column_name = 'active_org_id'",
    )
    .fetch_optional(&pool)
    .await
    .expect("the catalogue");
    assert!(column.is_none());
    CONTROL_PLANE
        .run(&pool)
        .await
        .expect("it applies again after going back");
}
