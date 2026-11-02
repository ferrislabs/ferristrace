use ferristrace_domain::{
    IngestKey, Orgs, RetentionDays, Scope, SecretHash, TenantDirectory, TenantId,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres, PostgresDirectory};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;
const SECRET: &str = "the-secret";

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

async fn org_on_plan(pool: &PgPool, plan_days: Option<i32>, org_days: Option<i32>) -> Uuid {
    let plan = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO plans (id, slug, name, retention_days) VALUES ($1, $1::text, 'p', $2)",
    )
    .bind(plan)
    .bind(plan_days)
    .execute(pool)
    .await
    .expect("a plan");
    let org = Uuid::new_v4();
    sqlx::query("INSERT INTO orgs (id, name, plan_id, retention_days) VALUES ($1, 'Acme', $2, $3)")
        .bind(org)
        .bind(plan)
        .bind(org_days)
        .execute(pool)
        .await
        .expect("an org");
    sqlx::query(
        "INSERT INTO api_keys (id, org_id, prefix, hash, secret_digest) \
         VALUES (gen_random_uuid(), $1, 'ft_live_abc', 'superseded', $2)",
    )
    .bind(org)
    .bind(SecretHash::of(SECRET).as_bytes().to_vec())
    .execute(pool)
    .await
    .expect("a key");
    org
}

fn days(count: u32) -> Option<RetentionDays> {
    RetentionDays::try_from(count).ok()
}

async fn resolved(directory: &PostgresDirectory<FixedClock>) -> Scope {
    directory
        .resolve(&IngestKey::from_plaintext(&format!("ft_live_abc.{SECRET}")))
        .await
        .expect("the key resolves")
}

fn directory(pool: &PgPool) -> PostgresDirectory<FixedClock> {
    PostgresDirectory::new(pool.clone(), FixedClock::at(NOW))
}

#[tokio::test]
async fn an_org_override_below_the_plan_cap_is_what_the_directory_resolves() {
    let Some(pool) = migrated("retention_override").await else {
        return;
    };
    let org = org_on_plan(&pool, Some(30), Some(7)).await;

    let scope = resolved(&directory(&pool)).await;

    assert_eq!(scope, Scope::of(TenantId::new(org)).retained_for(days(7)));
}

#[tokio::test]
async fn an_org_override_above_the_plan_cap_is_cut_to_the_cap() {
    let Some(pool) = migrated("retention_capped").await else {
        return;
    };
    org_on_plan(&pool, Some(30), Some(90)).await;

    assert_eq!(resolved(&directory(&pool)).await.retention, days(30));
}

#[tokio::test]
async fn an_override_on_a_plan_without_a_cap_stands() {
    let Some(pool) = migrated("retention_uncapped").await else {
        return;
    };
    org_on_plan(&pool, None, Some(365)).await;

    assert_eq!(resolved(&directory(&pool)).await.retention, days(365));
}

#[tokio::test]
async fn no_override_and_no_cap_leaves_the_deployment_default_to_the_sink() {
    let Some(pool) = migrated("retention_default").await else {
        return;
    };
    org_on_plan(&pool, None, None).await;

    let directory = directory(&pool).with_default_retention_days(30);

    assert_eq!(resolved(&directory).await.retention, None);
}

#[tokio::test]
async fn no_override_under_a_cap_below_the_default_resolves_to_the_cap() {
    let Some(pool) = migrated("retention_cap_below_default").await else {
        return;
    };
    org_on_plan(&pool, Some(10), None).await;

    let directory = directory(&pool).with_default_retention_days(30);

    assert_eq!(resolved(&directory).await.retention, days(10));
}

#[tokio::test]
async fn no_override_under_a_cap_above_the_default_keeps_the_default() {
    let Some(pool) = migrated("retention_cap_above_default").await else {
        return;
    };
    org_on_plan(&pool, Some(90), None).await;

    let directory = directory(&pool).with_default_retention_days(30);

    assert_eq!(resolved(&directory).await.retention, None);
}

#[tokio::test]
async fn the_orgs_repository_round_trips_the_override_and_reports_the_plan_cap() {
    let Some(pool) = migrated("retention_round_trip").await else {
        return;
    };
    let org = TenantId::new(org_on_plan(&pool, Some(30), None).await);
    let pg = Postgres::new(pool.clone(), FixedClock::at(NOW));
    let orgs = backend::orgs(&pg);

    let before = orgs.only(org).await.expect("read").expect("an org");
    orgs.set_retention(org, days(14)).await.expect("set");
    let set = orgs.only(org).await.expect("read").expect("an org");
    orgs.set_retention(org, None).await.expect("reset");
    let reset = orgs.only(org).await.expect("read").expect("an org");

    assert_eq!(before.retention, None);
    assert_eq!(before.retention_cap, days(30));
    assert_eq!(set.retention, days(14));
    assert_eq!(reset.retention, None);
}

#[tokio::test]
async fn the_schema_refuses_a_retention_of_zero() {
    let Some(pool) = migrated("retention_check").await else {
        return;
    };
    let org = org_on_plan(&pool, None, None).await;

    let refused = sqlx::query("UPDATE orgs SET retention_days = 0 WHERE id = $1")
        .bind(org)
        .execute(&pool)
        .await;

    assert!(refused.is_err());
}
