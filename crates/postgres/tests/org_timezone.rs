use ferristrace_domain::{OrgTimezone, Orgs, TenantId};
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

async fn an_org(pool: &PgPool) -> TenantId {
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, 'owner@example.com')")
        .bind(Uuid::new_v4())
        .execute(pool)
        .await
        .expect("inserting an account");
    backend::memberships(&backend_of(pool))
        .found("Acme", "owner@example.com")
        .await
        .expect("the organisation is founded")
}

#[tokio::test]
async fn an_organisation_reads_utc_until_it_is_given_a_zone() {
    let Some(pool) = control_plane("org_timezone_default").await else {
        return;
    };
    let org = an_org(&pool).await;

    let card = backend::orgs(&backend_of(&pool))
        .only(org)
        .await
        .expect("the read")
        .expect("the organisation");

    assert_eq!(card.timezone, OrgTimezone::utc());
}

#[tokio::test]
async fn a_zone_is_kept_and_read_back_for_that_organisation_only() {
    let Some(pool) = control_plane("org_timezone_kept").await else {
        return;
    };
    let org = an_org(&pool).await;
    let paris = OrgTimezone::try_from("Europe/Paris").expect("an IANA name");
    let pg = backend_of(&pool);

    backend::orgs(&pg)
        .set_timezone(org, &paris)
        .await
        .expect("the write");
    let card = backend::orgs(&pg)
        .only(org)
        .await
        .expect("the read")
        .expect("the organisation");

    assert_eq!(card.timezone, paris);
}

#[tokio::test]
async fn the_database_refuses_a_blank_zone() {
    let Some(pool) = control_plane("org_timezone_check").await else {
        return;
    };
    let org = an_org(&pool).await;

    let refused = sqlx::query("UPDATE orgs SET timezone = '' WHERE id = $1")
        .bind(org.as_uuid())
        .execute(&pool)
        .await;

    assert!(refused.is_err());
}
