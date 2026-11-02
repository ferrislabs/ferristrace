use ferristrace_domain::{AccountId, Membership, Memberships, OrgName, OrgRole, Orgs, TenantId};
use ferristrace_postgres::{CONTROL_PLANE, MembershipError, PostgresMemberships, PostgresOrgs};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

mod backend;

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

async fn an_account(pool: &PgPool, email: &str) -> AccountId {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'x')")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await
        .expect("inserting an account");
    AccountId::new(id)
}

async fn role_held(pool: &PgPool, org: TenantId, account: AccountId) -> Option<String> {
    sqlx::query("SELECT role::text AS role FROM org_members WHERE org_id = $1 AND account_id = $2")
        .bind(org.as_uuid())
        .bind(account.as_uuid())
        .fetch_optional(pool)
        .await
        .expect("reading the role")
        .map(|row| row.get("role"))
}

#[tokio::test]
async fn founding_an_organisation_makes_its_first_account_the_owner() {
    let Some(pool) = control_plane("memberships_founding").await else {
        return;
    };
    let account = an_account(&pool, "founder@example.com").await;
    let memberships = PostgresMemberships::new(pool.clone());

    let org = memberships
        .found("Acme", "founder@example.com")
        .await
        .expect("the organisation is founded");

    assert_eq!(
        role_held(&pool, org, account).await.as_deref(),
        Some("owner")
    );
    assert_eq!(
        memberships.of(account).await.expect("the memberships"),
        vec![Membership {
            org,
            name: "Acme".into(),
            role: OrgRole::Owner,
        }]
    );
}

#[tokio::test]
async fn founding_an_organisation_for_a_stranger_leaves_nothing_behind() {
    let Some(pool) = control_plane("memberships_stranger").await else {
        return;
    };
    let memberships = PostgresMemberships::new(pool.clone());

    let refused = memberships.found("Acme", "nobody@example.com").await;

    assert!(matches!(refused, Err(MembershipError::NoSuchAccount(_))));
    let orgs: i64 = sqlx::query("SELECT count(*) AS value FROM orgs")
        .fetch_one(&pool)
        .await
        .expect("counting the organisations")
        .get("value");
    assert_eq!(
        orgs, 0,
        "the organisation must not outlive the failed grant"
    );
}

#[tokio::test]
async fn a_granted_role_replaces_the_one_already_held() {
    let Some(pool) = control_plane("memberships_regrant").await else {
        return;
    };
    let owner = an_account(&pool, "owner@example.com").await;
    let joiner = an_account(&pool, "joiner@example.com").await;
    let memberships = PostgresMemberships::new(pool.clone());
    let org = memberships
        .found("Acme", "owner@example.com")
        .await
        .expect("the organisation is founded");

    memberships
        .grant(org, "joiner@example.com", OrgRole::Member)
        .await
        .expect("the first grant");
    memberships
        .grant(org, "joiner@example.com", OrgRole::Admin)
        .await
        .expect("the second grant");

    assert_eq!(
        role_held(&pool, org, joiner).await.as_deref(),
        Some("admin")
    );
    assert_eq!(
        role_held(&pool, org, owner).await.as_deref(),
        Some("owner"),
        "granting a role must not disturb the other members"
    );
}

#[tokio::test]
async fn an_account_carries_every_organisation_it_belongs_to() {
    let Some(pool) = control_plane("memberships_several").await else {
        return;
    };
    let account = an_account(&pool, "both@example.com").await;
    let memberships = PostgresMemberships::new(pool.clone());
    let first = memberships
        .found("First", "both@example.com")
        .await
        .expect("the first organisation");
    let second = memberships
        .found("Second", "both@example.com")
        .await
        .expect("the second organisation");

    let held = memberships.of(account).await.expect("the memberships");

    assert_eq!(held.len(), 2);
    assert!(held.iter().all(|one| one.role == OrgRole::Owner));
    let orgs: Vec<TenantId> = held.iter().map(|one| one.org).collect();
    assert!(orgs.contains(&first) && orgs.contains(&second));
}

#[tokio::test]
async fn an_account_in_no_organisation_carries_no_role() {
    let Some(pool) = control_plane("memberships_none").await else {
        return;
    };
    let lonely = an_account(&pool, "lonely@example.com").await;

    let held = PostgresMemberships::new(pool)
        .of(lonely)
        .await
        .expect("the memberships");

    assert!(held.is_empty());
}

#[tokio::test]
async fn deleting_an_organisation_takes_its_memberships_with_it() {
    let Some(pool) = control_plane("memberships_cascade").await else {
        return;
    };
    let account = an_account(&pool, "owner@example.com").await;
    let memberships = PostgresMemberships::new(pool.clone());
    let org = memberships
        .found("Acme", "owner@example.com")
        .await
        .expect("the organisation is founded");

    sqlx::query("DELETE FROM orgs WHERE id = $1")
        .bind(org.as_uuid())
        .execute(&pool)
        .await
        .expect("deleting the organisation");

    assert!(
        memberships
            .of(account)
            .await
            .expect("the memberships")
            .is_empty()
    );
}

#[tokio::test]
async fn an_organisation_reads_back_with_its_name_its_plan_and_when_it_was_founded() {
    let Some(pool) = control_plane("memberships_card").await else {
        return;
    };
    an_account(&pool, "card@example.com").await;
    let org = PostgresMemberships::new(pool.clone())
        .found("Acme", "card@example.com")
        .await
        .expect("the organisation is founded");

    let card = PostgresOrgs::new(pool.clone())
        .only(org)
        .await
        .expect("the organisation is read")
        .expect("a row carries it");

    assert_eq!(card.id, org);
    assert_eq!(&*card.name, "Acme");
    assert_eq!(
        &*card.plan, "Self-hosted",
        "the plan the migration seeds every self-hosted deployment with"
    );
    assert!(
        card.created_at_unix_nanos > 1_600_000_000_000_000_000,
        "the column holds nanoseconds, not seconds: {}",
        card.created_at_unix_nanos
    );
}

#[tokio::test]
async fn renaming_an_organisation_leaves_its_id_and_its_founding_instant_alone() {
    let Some(pool) = control_plane("memberships_rename").await else {
        return;
    };
    an_account(&pool, "rename@example.com").await;
    let org = PostgresMemberships::new(pool.clone())
        .found("Acme", "rename@example.com")
        .await
        .expect("the organisation is founded");
    let orgs = PostgresOrgs::new(pool.clone());
    let before = orgs
        .only(org)
        .await
        .expect("the organisation is read")
        .expect("a row carries it");

    orgs.rename(org, &OrgName::try_from("Acme Payments").expect("a name"))
        .await
        .expect("the organisation is renamed");

    let after = orgs
        .only(org)
        .await
        .expect("the organisation is read")
        .expect("a row carries it");
    assert_eq!(&*after.name, "Acme Payments");
    assert_eq!(after.id, before.id);
    assert_eq!(after.created_at_unix_nanos, before.created_at_unix_nanos);
}

#[tokio::test]
async fn an_id_no_organisation_carries_reads_as_nothing() {
    let Some(pool) = control_plane("memberships_no_card").await else {
        return;
    };

    let found = PostgresOrgs::new(pool)
        .only(TenantId::new(Uuid::new_v4()))
        .await
        .expect("the organisation is read");

    assert!(found.is_none());
}
