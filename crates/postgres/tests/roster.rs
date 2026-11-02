use ferristrace_domain::{AccountId, Admitted, Email, Member, OrgRole, Roll, SecretHash, TenantId};
use ferristrace_infra::{Argon2Hasher, FixedClock};
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

const NOW: u64 = 1_700_000_000;
const A_DAY: i64 = 24 * 60 * 60 * 1_000_000_000;

#[macro_use]
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

fn roster_backend(pool: &PgPool) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(NOW))
        .with_hasher(Argon2Hasher::with_cost(8, 1, 1).expect("a cheap hasher"))
}

async fn an_account(pool: &PgPool, email: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'chosen')")
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

fn a_digest(seed: u8) -> SecretHash {
    SecretHash::from_bytes([seed; 32])
}

async fn hash_held(pool: &PgPool, email: &str) -> Option<String> {
    sqlx::query("SELECT password_hash AS password_hash FROM accounts WHERE email = $1")
        .bind(email)
        .fetch_optional(pool)
        .await
        .expect("reading the account")
        .and_then(|row| row.get("password_hash"))
}

async fn resets_for(pool: &PgPool, account: Uuid) -> Vec<(Vec<u8>, i64, i64)> {
    sqlx::query(
        "SELECT digest AS digest, created_at AS created_at, expires_at AS expires_at \
         FROM password_resets WHERE account_id = $1",
    )
    .bind(account)
    .fetch_all(pool)
    .await
    .expect("reading the resets")
    .into_iter()
    .map(|row| {
        (
            row.get("digest"),
            row.get("created_at"),
            row.get("expires_at"),
        )
    })
    .collect()
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
async fn an_unknown_address_is_given_an_account_a_membership_and_one_way_in() {
    let Some(pool) = control_plane("roster_admit_new").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;

    let admitted = backend::roster(&roster_backend(&pool))
        .admit(org, &an_address("new"), OrgRole::Member, a_digest(7))
        .await
        .expect("the admission");

    let Admitted::Awaiting(member) = admitted else {
        panic!("an address nobody held must be awaiting a password: {admitted:?}");
    };
    assert_eq!(member.email.as_str(), "new@example.com");
    assert_eq!(member.role, OrgRole::Member);

    let held = hash_held(&pool, "new@example.com")
        .await
        .expect("the row satisfies the constraint that every account carries a way in");
    assert!(held.starts_with("$argon2"), "{held}");

    let resets = resets_for(&pool, member.account.as_uuid()).await;
    assert_eq!(resets.len(), 1);
    let (digest, created_at, expires_at) = &resets[0];
    assert_eq!(digest.as_slice(), a_digest(7).as_bytes().as_slice());
    assert_eq!(
        *created_at,
        i64::try_from(NOW).expect("a date") * 1_000_000_000
    );
    assert_eq!(
        *expires_at - *created_at,
        A_DAY,
        "an invitation lasts as long as one handed over by the command line"
    );
}

#[tokio::test]
async fn an_address_that_already_has_an_account_joins_without_a_way_in() {
    let Some(pool) = control_plane("roster_admit_known").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let standing = an_account(&pool, "known@example.com").await;

    let admitted = backend::roster(&roster_backend(&pool))
        .admit(org, &an_address("known"), OrgRole::Admin, a_digest(9))
        .await
        .expect("the admission");

    let Admitted::Standing(member) = admitted else {
        panic!("an address somebody already holds must join standing: {admitted:?}");
    };
    assert_eq!(member.account.as_uuid(), standing);
    assert_eq!(member.role, OrgRole::Admin);
    assert!(
        resets_for(&pool, standing).await.is_empty(),
        "an administrator must not be handed the password of an account it does not own"
    );
    assert_eq!(
        hash_held(&pool, "known@example.com").await.as_deref(),
        Some("chosen"),
        "the password it chose is untouched"
    );
}

#[tokio::test]
async fn admitting_an_address_twice_leaves_one_account_one_membership_and_one_way_in() {
    let Some(pool) = control_plane("roster_admit_twice").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let first = backend::roster(&roster_backend(&pool))
        .admit(org, &an_address("new"), OrgRole::Member, a_digest(1))
        .await
        .expect("the first admission");
    let Admitted::Awaiting(member) = first else {
        panic!("the first admission must create the account: {first:?}");
    };

    let again = backend::roster(&roster_backend(&pool))
        .admit(org, &an_address("new"), OrgRole::Admin, a_digest(2))
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
    assert_eq!(
        resets_for(&pool, member.account.as_uuid()).await.len(),
        1,
        "the refused admission must leave no second way in behind"
    );
}

#[tokio::test]
async fn deleting_an_organisation_takes_the_roll_with_it_and_leaves_the_accounts() {
    let Some(pool) = control_plane("roster_cascade").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    backend::roster(&roster_backend(&pool))
        .admit(org, &an_address("new"), OrgRole::Member, a_digest(3))
        .await
        .expect("the admission");

    sqlx::query("DELETE FROM orgs WHERE id = $1")
        .bind(org.as_uuid())
        .execute(&pool)
        .await
        .expect("deleting the organisation");

    assert!(
        backend::roster(&roster_backend(&pool))
            .of(org)
            .await
            .expect("the roll is read")
            .is_empty()
    );
    assert_eq!(accounts_with(&pool, "new@example.com").await, 1);
}

#[tokio::test]
async fn the_roll_carries_the_name_an_account_was_given() {
    let Some(pool) = control_plane("roster_named").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let joiner = an_account(&pool, "joiner@example.com").await;
    backend::memberships(&Postgres::new(pool.clone(), FixedClock::at(NOW)))
        .grant(org, "joiner@example.com", OrgRole::Member)
        .await
        .expect("the grant");
    named(&pool, joiner, "Ines Martin").await;

    let roll = backend::roster(&roster_backend(&pool))
        .of(org)
        .await
        .expect("the roll is read");

    assert_eq!(
        member_at(&roll, "joiner@example.com").name.as_deref(),
        Some("Ines Martin")
    );
    assert_eq!(
        member_at(&roll, "owner@example.com").name,
        None,
        "an account nobody has named is listed under its address"
    );
}

#[tokio::test]
async fn an_admitted_standing_account_brings_its_name_with_it() {
    let Some(pool) = control_plane("roster_admit_named").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let standing = an_account(&pool, "known@example.com").await;
    named(&pool, standing, "Ines Martin").await;

    let admitted = backend::roster(&roster_backend(&pool))
        .admit(org, &an_address("known"), OrgRole::Admin, a_digest(9))
        .await
        .expect("the admission");

    let Admitted::Standing(member) = admitted else {
        panic!("an address somebody already holds must join standing: {admitted:?}");
    };
    assert_eq!(member.name.as_deref(), Some("Ines Martin"));
}

#[tokio::test]
async fn an_account_created_by_an_invitation_carries_no_name() {
    let Some(pool) = control_plane("roster_admit_unnamed").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;

    let admitted = backend::roster(&roster_backend(&pool))
        .admit(org, &an_address("stranger"), OrgRole::Member, a_digest(4))
        .await
        .expect("the admission");

    let Admitted::Awaiting(member) = admitted else {
        panic!("an unknown address must be given an account: {admitted:?}");
    };
    assert_eq!(member.name, None, "nobody has written a name for them yet");
}

async fn named(pool: &PgPool, account: Uuid, name: &str) {
    sqlx::query("UPDATE accounts SET name = $2 WHERE id = $1")
        .bind(account)
        .bind(name)
        .execute(pool)
        .await
        .expect("naming an account");
}
