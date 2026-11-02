use ferristrace_domain::{AccountId, Email, Member, OrgRole, Roll, TenantId};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

const NOW: u64 = 1_700_000_000;

#[macro_use]
mod backend;

#[derive(Debug, PartialEq, Eq)]
enum Admitted {
    Created(Member),
    Standing(Member),
    AlreadyIn,
}

async fn admit(
    pool: &PgPool,
    org: TenantId,
    email: &Email,
    role: OrgRole,
) -> Result<Admitted, ferristrace_domain::StoreError> {
    let backing = roster_backend(pool);
    let roster = backend::roster(&backing);
    let standing = roster.standing_of(email).await?;
    let (holder, created) = if let Some(holder) = standing {
        (holder, false)
    } else {
        let account = roster.create_account(email).await?;
        let holder = ferristrace_domain::organisation::Holder {
            account,
            name: None,
            photo: None,
        };
        (holder, true)
    };
    let Some(joined) = roster.join(org, holder.account, role).await? else {
        return Ok(Admitted::AlreadyIn);
    };
    let member = Member {
        account: holder.account,
        email: email.clone(),
        name: holder.name,
        photo: holder.photo,
        role: joined.role,
        joined_at_unix_nanos: joined.joined_at_unix_nanos,
    };
    if !created {
        return Ok(Admitted::Standing(member));
    }
    Ok(Admitted::Created(member))
}

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

fn roster_backend(pool: &PgPool) -> Postgres<FixedClock> {
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

async fn an_org(pool: &PgPool, owner: &str) -> TenantId {
    an_account(pool, owner).await;
    backend::memberships(&Postgres::new(pool.clone(), FixedClock::at(NOW)))
        .found("Acme", owner)
        .await
        .expect("the organisation is founded")
}

fn an_address(local: &str) -> Email {
    Email::try_from(format!("{local}@example.com").as_str()).expect("an address")
}

async fn links_of(pool: &PgPool, account: Uuid) -> i64 {
    sqlx::query("SELECT count(*) AS value FROM identity_links WHERE account_id = $1")
        .bind(account)
        .fetch_one(pool)
        .await
        .expect("counting the links")
        .get("value")
}

async fn accounts_with(pool: &PgPool, email: &str) -> i64 {
    sqlx::query("SELECT count(*) AS value FROM accounts WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .expect("counting the accounts")
        .get("value")
}

fn member_at(roll: &[Member], email: &str) -> Member {
    roll.iter()
        .find(|member| member.email.as_str() == email)
        .cloned()
        .unwrap_or_else(|| panic!("{email} is on the roll"))
}

#[tokio::test]
async fn the_roll_reads_back_in_the_order_the_screen_wants_it() {
    let Some(pool) = control_plane("roster_order").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let memberships = backend::memberships(&pg);
    for (email, role) in [
        ("zoe@example.com", OrgRole::Admin),
        ("ada@example.com", OrgRole::Member),
        ("abe@example.com", OrgRole::Member),
    ] {
        an_account(&pool, email).await;
        memberships
            .grant(org, email, role)
            .await
            .expect("the grant");
    }

    let roll = backend::roster(&roster_backend(&pool))
        .of(org)
        .await
        .expect("the roll is read");

    let addresses: Vec<&str> = roll.iter().map(|member| member.email.as_str()).collect();
    assert_eq!(
        addresses,
        vec![
            "owner@example.com",
            "zoe@example.com",
            "abe@example.com",
            "ada@example.com",
        ],
        "owner, then administrators, then everybody else by address"
    );
    assert!(
        roll[0].joined_at_unix_nanos > 1_600_000_000_000_000_000,
        "the column holds nanoseconds, not seconds: {}",
        roll[0].joined_at_unix_nanos
    );
}

#[tokio::test]
async fn the_roll_of_an_organisation_with_one_member_holds_only_that_one() {
    let Some(pool) = control_plane("roster_alone").await else {
        return;
    };
    let org = an_org(&pool, "alone@example.com").await;
    let other = an_org(&pool, "elsewhere@example.com").await;

    let roll = backend::roster(&roster_backend(&pool))
        .of(org)
        .await
        .expect("the roll is read");

    assert_eq!(roll.len(), 1);
    assert_eq!(roll[0].email.as_str(), "alone@example.com");
    assert_eq!(roll[0].role, OrgRole::Owner);
    assert_eq!(
        backend::roster(&roster_backend(&pool))
            .of(other)
            .await
            .expect("the other roll is read")
            .len(),
        1,
        "one organisation must not read the members of another"
    );
}

#[tokio::test]
async fn a_granted_role_replaces_the_one_held_and_answers_with_the_member() {
    let Some(pool) = control_plane("roster_grant").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let joiner = an_account(&pool, "joiner@example.com").await;
    backend::memberships(&Postgres::new(pool.clone(), FixedClock::at(NOW)))
        .grant(org, "joiner@example.com", OrgRole::Member)
        .await
        .expect("the first grant");

    let changed = backend::roster(&roster_backend(&pool))
        .grant(org, AccountId::new(joiner), OrgRole::Admin)
        .await
        .expect("the role is changed")
        .expect("the membership was there");

    assert_eq!(changed.role, OrgRole::Admin);
    assert_eq!(changed.email.as_str(), "joiner@example.com");
    let roll = backend::roster(&roster_backend(&pool))
        .of(org)
        .await
        .expect("the roll is read");
    assert_eq!(member_at(&roll, "joiner@example.com").role, OrgRole::Admin);
    assert_eq!(
        member_at(&roll, "owner@example.com").role,
        OrgRole::Owner,
        "changing one role must not disturb the others"
    );
}

#[tokio::test]
async fn granting_a_role_in_an_organisation_that_holds_no_such_membership_changes_nothing() {
    let Some(pool) = control_plane("roster_no_grant").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let stranger = an_account(&pool, "stranger@example.com").await;

    let changed = backend::roster(&roster_backend(&pool))
        .grant(org, AccountId::new(stranger), OrgRole::Admin)
        .await
        .expect("the control plane answers");

    assert!(changed.is_none());
    assert_eq!(
        backend::roster(&roster_backend(&pool))
            .of(org)
            .await
            .expect("the roll is read")
            .len(),
        1
    );
}

#[tokio::test]
async fn a_membership_is_removed_once_and_then_there_is_nothing_left_to_remove() {
    let Some(pool) = control_plane("roster_remove").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let joiner = an_account(&pool, "joiner@example.com").await;
    backend::memberships(&Postgres::new(pool.clone(), FixedClock::at(NOW)))
        .grant(org, "joiner@example.com", OrgRole::Member)
        .await
        .expect("the grant");
    let account = AccountId::new(joiner);

    let first = backend::roster(&roster_backend(&pool))
        .remove(org, account)
        .await
        .expect("the removal");
    let again = backend::roster(&roster_backend(&pool))
        .remove(org, account)
        .await
        .expect("the second removal");

    assert!(first);
    assert!(!again);
    assert_eq!(
        accounts_with(&pool, "joiner@example.com").await,
        1,
        "losing a membership is not losing the account"
    );
}

#[tokio::test]
async fn an_unknown_address_is_given_an_account_and_a_membership() {
    let Some(pool) = control_plane("roster_admit_new").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;

    let admitted = admit(&pool, org, &an_address("new"), OrgRole::Member)
        .await
        .expect("the admission");

    let Admitted::Created(member) = admitted else {
        panic!("an address nobody held must get an account: {admitted:?}");
    };
    assert_eq!(member.email.as_str(), "new@example.com");
    assert_eq!(member.role, OrgRole::Member);
    assert_eq!(accounts_with(&pool, "new@example.com").await, 1);
    assert_eq!(links_of(&pool, member.account.as_uuid()).await, 0);
}

#[tokio::test]
async fn an_address_that_already_has_an_account_joins_that_account() {
    let Some(pool) = control_plane("roster_admit_known").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let standing = an_account(&pool, "known@example.com").await;

    let admitted = admit(&pool, org, &an_address("known"), OrgRole::Admin)
        .await
        .expect("the admission");

    let Admitted::Standing(member) = admitted else {
        panic!("an address somebody already holds must join standing: {admitted:?}");
    };
    assert_eq!(member.account.as_uuid(), standing);
    assert_eq!(member.role, OrgRole::Admin);
    assert_eq!(accounts_with(&pool, "known@example.com").await, 1);
}

#[tokio::test]
async fn admitting_an_address_twice_leaves_one_account_and_one_membership() {
    let Some(pool) = control_plane("roster_admit_twice").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let first = admit(&pool, org, &an_address("new"), OrgRole::Member)
        .await
        .expect("the first admission");
    assert!(matches!(first, Admitted::Created(_)), "{first:?}");

    let again = admit(&pool, org, &an_address("new"), OrgRole::Admin)
        .await
        .expect("the second admission");

    assert_eq!(
        again,
        Admitted::AlreadyIn,
        "a second invitation must not quietly change the role somebody holds"
    );
    assert_eq!(accounts_with(&pool, "new@example.com").await, 1);
    let roll = backend::roster(&roster_backend(&pool))
        .of(org)
        .await
        .expect("the roll is read");
    assert_eq!(member_at(&roll, "new@example.com").role, OrgRole::Member);
}
