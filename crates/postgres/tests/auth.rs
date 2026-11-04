use ferristrace_domain::{
    AccountId, AccountName, Accounts, Email, PhotoKind, Portrait, ProvisionPolicy, SessionError,
    SessionStore, SessionToken,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{AccountError, CONTROL_PLANE, Postgres};

use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;

fn at(pool: &PgPool, instant: u64) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(instant))
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

async fn an_account(pool: &PgPool, email: &str) -> AccountId {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await
        .expect("an account");
    AccountId::new(id)
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query(AssertSqlSafe(format!("SELECT count(*) AS n FROM {table}")))
        .fetch_one(pool)
        .await
        .expect("a count")
        .get("n")
}

#[tokio::test]
async fn a_first_call_provisions_an_account_and_a_second_reuses_it() {
    let Some(pool) = control_plane("auth_provisions").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let sessions = backend::sessions(&pg);
    let token = SessionToken::for_subject("newcomer");

    let first = sessions.resolve(&token).await.expect("it provisions");
    let second = sessions
        .resolve(&SessionToken::for_subject("newcomer"))
        .await
        .expect("it reuses");

    assert_eq!(first.account, second.account);
    assert_eq!(first.email.as_str(), "newcomer@ferristrace.test");
    assert_eq!(first.org, None);
    assert_eq!(first.expires_at, token.expires_at());
    assert_eq!(count(&pool, "accounts").await, 1);
    assert_eq!(count(&pool, "identity_links").await, 1);
}

#[tokio::test]
async fn two_first_calls_at_once_settle_on_one_account() {
    let Some(pool) = control_plane("auth_provisions_race").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let token = SessionToken::for_subject("newcomer");

    let sessions = backend::sessions(&pg);

    let (one, other) = tokio::join!(sessions.resolve(&token), sessions.resolve(&token));

    assert_eq!(
        one.expect("the first resolves").account,
        other.expect("the second resolves").account
    );
    assert_eq!(count(&pool, "accounts").await, 1);
}

#[tokio::test]
async fn an_unknown_identity_is_refused_when_provisioning_is_off() {
    let Some(pool) = control_plane("auth_provisioning_off").await else {
        return;
    };
    let pg = at(&pool, NOW).with_provision_policy(ProvisionPolicy {
        auto_provision: false,
        link_by_email: false,
    });

    let refused = backend::sessions(&pg)
        .resolve(&SessionToken::for_subject("stranger"))
        .await;

    assert_eq!(refused, Err(SessionError::Unknown));
    assert_eq!(count(&pool, "accounts").await, 0);
}

#[tokio::test]
async fn a_linked_identity_resolves_to_its_account_and_its_active_org() {
    let Some(pool) = control_plane("auth_linked").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let org = backend::memberships(&pg)
        .found("Acme", "person@example.com")
        .await
        .expect("an organisation");
    let token = backend::signed_in(&pool, account, Some(org)).await;

    let session = backend::sessions(&pg)
        .resolve(&token)
        .await
        .expect("it resolves");

    assert_eq!(session.account, account);
    assert_eq!(session.org, Some(org));
    assert_eq!(count(&pool, "accounts").await, 1);
}

#[tokio::test]
async fn an_address_is_linked_only_when_the_policy_and_the_provider_agree() {
    let Some(pool) = control_plane("auth_link_by_email").await else {
        return;
    };
    let account = an_account(&pool, "newcomer@ferristrace.test").await;
    let linking = at(&pool, NOW).with_provision_policy(ProvisionPolicy {
        auto_provision: false,
        link_by_email: true,
    });

    let session = backend::sessions(&linking)
        .resolve(&SessionToken::for_subject("newcomer"))
        .await
        .expect("it links by address");

    assert_eq!(session.account, account);
    assert_eq!(count(&pool, "identity_links").await, 1);
}

#[tokio::test]
async fn an_address_carries_at_most_one_account() {
    let Some(pool) = control_plane("accounts_taken").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let address = Email::try_from("twice@example.com").expect("an address");
    let accounts = backend::accounts(&pg);
    accounts.create(&address).await.expect("the first account");

    let refused = accounts.create(&address).await;

    assert!(matches!(refused, Err(AccountError::Taken)));
}

#[tokio::test]
async fn an_account_carries_no_name_until_somebody_writes_one() {
    let Some(pool) = control_plane("auth_unnamed").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let sessions = backend::sessions(&pg);

    let token = backend::signed_in(&pool, account, None).await;
    let session = sessions.resolve(&token).await.expect("it resolves");

    assert_eq!(session.name, None);
    assert_eq!(session.email.as_str(), "person@example.com");
}

#[tokio::test]
async fn a_chosen_name_travels_on_the_session() {
    let Some(pool) = control_plane("auth_named").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let accounts = backend::accounts(&pg);
    let sessions = backend::sessions(&pg);
    let name = AccountName::try_from("Ines Martin").expect("an admitted name");

    accounts
        .rename(account, Some(&name))
        .await
        .expect("it renames");
    let token = backend::signed_in(&pool, account, None).await;
    let session = sessions.resolve(&token).await.expect("it resolves");

    assert_eq!(session.name.as_deref(), Some("Ines Martin"));
}

#[tokio::test]
async fn emptying_the_name_writes_a_null_rather_than_a_blank() {
    let Some(pool) = control_plane("auth_unnamed_again").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let accounts = backend::accounts(&pg);
    let name = AccountName::try_from("Ines Martin").expect("an admitted name");
    accounts
        .rename(account, Some(&name))
        .await
        .expect("it renames");

    accounts.rename(account, None).await.expect("it empties");

    let held: Option<String> = sqlx::query("SELECT name AS name FROM accounts WHERE id = $1")
        .bind(account.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("the account")
        .get("name");
    assert_eq!(
        held, None,
        "an account with no name holds one null, not two spellings of none"
    );
}

#[tokio::test]
async fn the_column_refuses_a_name_that_is_only_space() {
    let Some(pool) = control_plane("auth_blank_name").await else {
        return;
    };
    let account = an_account(&pool, "person@example.com").await;

    let refused = sqlx::query("UPDATE accounts SET name = $2 WHERE id = $1")
        .bind(account.as_uuid())
        .bind("   ")
        .execute(&pool)
        .await;

    assert!(refused.is_err(), "the column admitted a blank name");
}

#[tokio::test]
async fn a_portrait_travels_on_the_session() {
    let Some(pool) = control_plane("auth_portrait").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let accounts = backend::accounts(&pg);
    let sessions = backend::sessions(&pg);
    let portrait = Portrait {
        token: "deadbeefdeadbeef".into(),
        kind: PhotoKind::Png,
    };

    accounts
        .set_portrait(account, Some(&portrait))
        .await
        .expect("it is kept");
    let token = backend::signed_in(&pool, account, None).await;
    let session = sessions.resolve(&token).await.expect("it resolves");

    assert_eq!(session.photo.as_ref(), Some(&portrait));
    assert_eq!(
        accounts.portrait_of(account).await.expect("it is read"),
        Some(portrait)
    );
}

#[tokio::test]
async fn a_dropped_portrait_leaves_both_columns_null() {
    let Some(pool) = control_plane("auth_portrait_gone").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let accounts = backend::accounts(&pg);
    accounts
        .set_portrait(
            account,
            Some(&Portrait {
                token: "deadbeefdeadbeef".into(),
                kind: PhotoKind::Jpeg,
            }),
        )
        .await
        .expect("it is kept");

    accounts.set_portrait(account, None).await.expect("it goes");

    assert_eq!(
        accounts.portrait_of(account).await.expect("it is read"),
        None
    );
    let held: (Option<String>, Option<String>) = sqlx::query(
        "SELECT photo_token AS photo_token, photo_type AS photo_type FROM accounts WHERE id = $1",
    )
    .bind(account.as_uuid())
    .fetch_one(&pool)
    .await
    .map(|row| (row.get("photo_token"), row.get("photo_type")))
    .expect("the account");
    assert_eq!(held, (None, None));
}

#[tokio::test]
async fn the_columns_refuse_half_a_photo() {
    let Some(pool) = control_plane("auth_portrait_half").await else {
        return;
    };
    let account = an_account(&pool, "person@example.com").await;

    let refused = sqlx::query("UPDATE accounts SET photo_token = 'deadbeef' WHERE id = $1")
        .bind(account.as_uuid())
        .execute(&pool)
        .await;

    assert!(
        refused.is_err(),
        "the columns admitted a token with no type"
    );
}

#[tokio::test]
async fn a_type_the_build_does_not_know_reads_as_no_photo() {
    let Some(pool) = control_plane("auth_portrait_odd").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    sqlx::query(
        "UPDATE accounts SET photo_token = 'deadbeef', photo_type = 'image/gif' WHERE id = $1",
    )
    .bind(account.as_uuid())
    .execute(&pool)
    .await
    .expect("the control plane holds what it is given");

    let read = backend::accounts(&pg)
        .portrait_of(account)
        .await
        .expect("it is read");

    assert_eq!(read, None);
}
