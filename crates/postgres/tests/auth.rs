use ferristrace_domain::{
    AccountId, AccountName, Accounts, Agent, CodeVerifier, Email, FlowError, OidcFlows,
    PendingFlow, PhotoKind, Portrait, SessionError, SessionId, SessionLifetime, SessionRoll,
    SessionStore,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{AccountError, CONTROL_PLANE, Postgres};
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;

fn lifetime() -> SessionLifetime {
    SessionLifetime {
        idle_seconds: 100,
        hard_seconds: 1000,
    }
}

fn at(pool: &PgPool, instant: u64) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(instant)).with_session_lifetime(lifetime())
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

#[tokio::test]
async fn a_session_resolves_to_its_account() {
    let Some(pool) = control_plane("auth_session").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let sessions = backend::sessions(&pg);

    let token = sessions
        .issue(account, None, None)
        .await
        .expect("a session");
    let session = sessions.resolve(&token).await.expect("it resolves");

    assert!(token.looks_like_one());
    assert_eq!(session.account, account);
    assert_eq!(session.org, None);
}

#[tokio::test]
async fn a_revoked_session_resolves_no_longer() {
    let Some(pool) = control_plane("auth_revoked").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let sessions = backend::sessions(&pg);
    let token = sessions
        .issue(account, None, None)
        .await
        .expect("a session");

    sessions.revoke(&token).await.expect("it revokes");

    assert_eq!(sessions.resolve(&token).await, Err(SessionError::Unknown));
}

#[tokio::test]
async fn an_expired_session_resolves_no_longer() {
    let Some(pool) = control_plane("auth_expired").await else {
        return;
    };
    let account = an_account(&pool, "person@example.com").await;
    let early_pg = at(&pool, NOW);
    let early = backend::sessions(&early_pg);
    let late_pg = at(&pool, NOW + 101);
    let late = backend::sessions(&late_pg);
    let token = early.issue(account, None, None).await.expect("a session");

    assert_eq!(late.resolve(&token).await, Err(SessionError::Unknown));
}

#[tokio::test]
async fn resolving_slides_the_expiry_but_not_past_the_hard_life() {
    let Some(pool) = control_plane("auth_sliding").await else {
        return;
    };
    let account = an_account(&pool, "person@example.com").await;
    let early_pg = at(&pool, NOW);
    let early = backend::sessions(&early_pg);
    let token = early.issue(account, None, None).await.expect("a session");

    let mut expiry = 0;
    for step in (50..=950).step_by(90) {
        let store_pg = at(&pool, NOW + step);
        let store = backend::sessions(&store_pg);
        let session = store
            .resolve(&token)
            .await
            .expect("a session kept in use stays alive");
        expiry = session.expires_at.get();
        if step == 50 {
            assert_eq!(expiry, NOW + 150, "the first sighting slides one window");
        }
    }

    assert_eq!(
        expiry,
        NOW + 1000,
        "the hard life caps the window however often it is used"
    );
}

#[tokio::test]
async fn pruning_removes_only_what_has_expired() {
    let Some(pool) = control_plane("auth_prune").await else {
        return;
    };
    let account = an_account(&pool, "person@example.com").await;
    let early_pg = at(&pool, NOW);
    let early = backend::sessions(&early_pg);
    early.issue(account, None, None).await.expect("a session");
    let later_pg = at(&pool, NOW + 500);
    let later = backend::sessions(&later_pg);
    later
        .issue(account, None, None)
        .await
        .expect("a second session");

    let removed = backend::sessions(&at(&pool, NOW + 101))
        .prune()
        .await
        .expect("a prune");

    let left: i64 = sqlx::query("SELECT count(*) AS n FROM sessions")
        .fetch_one(&pool)
        .await
        .expect("counting sessions")
        .get("n");
    assert_eq!((removed, left), (1, 1));
}

fn a_flow(state: &str) -> PendingFlow {
    PendingFlow {
        state: state.to_owned(),
        nonce: "the-nonce".to_owned(),
        verifier: CodeVerifier::from_plaintext("the-verifier"),
        redirect_uri: "https://app.example.com/callback".to_owned(),
    }
}

#[tokio::test]
async fn an_attempt_comes_back_once_and_only_once() {
    let Some(pool) = control_plane("flows_once").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let flows = backend::flows(&pg);
    flows.begin(&a_flow("state-1")).await.expect("it is kept");

    let first = flows.take("state-1").await.expect("it comes back");
    let second = flows.take("state-1").await;

    assert_eq!(first.nonce, "the-nonce");
    assert_eq!(first.verifier.exposed(), "the-verifier");
    assert_eq!(first.redirect_uri, "https://app.example.com/callback");
    assert_eq!(
        second,
        Err(FlowError::Unknown),
        "a replay must find nothing"
    );
}

#[tokio::test]
async fn an_unknown_state_carries_no_attempt() {
    let Some(pool) = control_plane("flows_unknown").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let flows = backend::flows(&pg);

    assert_eq!(flows.take("never-issued").await, Err(FlowError::Unknown));
}

#[tokio::test]
async fn an_attempt_past_its_window_is_gone() {
    let Some(pool) = control_plane("flows_expired").await else {
        return;
    };
    let early_pg = at(&pool, NOW).with_flow_window(Duration::from_secs(60));
    let early = backend::flows(&early_pg);
    let late_pg = at(&pool, NOW + 61).with_flow_window(Duration::from_secs(60));
    let late = backend::flows(&late_pg);
    early.begin(&a_flow("state-1")).await.expect("it is kept");

    assert_eq!(late.take("state-1").await, Err(FlowError::Unknown));
}

#[tokio::test]
async fn two_attempts_do_not_shadow_each_other() {
    let Some(pool) = control_plane("flows_two").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let flows = backend::flows(&pg);
    flows.begin(&a_flow("state-1")).await.expect("the first");
    let mut second = a_flow("state-2");
    second.nonce = "another-nonce".to_owned();
    flows.begin(&second).await.expect("the second");

    let taken = flows.take("state-2").await.expect("the second comes back");

    assert_eq!(taken.nonce, "another-nonce");
    assert!(
        flows.take("state-1").await.is_ok(),
        "the first is untouched"
    );
}

#[tokio::test]
async fn pruning_removes_only_attempts_past_their_window() {
    let Some(pool) = control_plane("flows_prune").await else {
        return;
    };
    let early_pg = at(&pool, NOW).with_flow_window(Duration::from_secs(60));
    let early = backend::flows(&early_pg);
    early.begin(&a_flow("state-1")).await.expect("the first");
    let later_pg = at(&pool, NOW + 500).with_flow_window(Duration::from_secs(60));
    let later = backend::flows(&later_pg);
    later.begin(&a_flow("state-2")).await.expect("the second");

    let removed = backend::flows(&at(&pool, NOW + 61))
        .prune()
        .await
        .expect("a prune");

    assert_eq!(removed, 1);
    assert!(later.take("state-2").await.is_ok(), "the live one stayed");
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

    let token = sessions
        .issue(account, None, None)
        .await
        .expect("a session");
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
    let token = sessions
        .issue(account, None, None)
        .await
        .expect("a session");
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
    let token = sessions
        .issue(account, None, None)
        .await
        .expect("a session");
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

#[tokio::test]
async fn the_roll_names_every_live_session_and_marks_the_caller() {
    let Some(pool) = control_plane("auth_roll").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let sessions = backend::sessions(&pg);
    let phone = Agent::new("Mozilla/5.0 (iPhone)").expect("an agent");
    let here = sessions
        .issue(account, None, Some(&phone))
        .await
        .expect("a session");
    let elsewhere = sessions.issue(account, None, None).await.expect("another");
    sessions.revoke(&elsewhere).await.expect("it revokes");
    let third = sessions.issue(account, None, None).await.expect("a third");

    let roll = sessions.live(account, &here).await.expect("a roll");

    assert_eq!(roll.len(), 2, "a revoked session is not live");
    let mine = roll.iter().find(|one| one.current).expect("the caller");
    assert_eq!(mine.from.as_ref().map(Agent::as_str), Some(phone.as_str()));
    assert_eq!(mine.created_at.get(), NOW);
    assert_eq!(roll.iter().filter(|one| !one.current).count(), 1);
    assert!(sessions.resolve(&third).await.is_ok());
}

#[tokio::test]
async fn one_session_is_revoked_by_its_public_id_and_the_others_are_left() {
    let Some(pool) = control_plane("auth_revoke_one").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let sessions = backend::sessions(&pg);
    let here = sessions
        .issue(account, None, None)
        .await
        .expect("a session");
    let doomed = sessions.issue(account, None, None).await.expect("another");
    let roll = sessions.live(account, &here).await.expect("a roll");
    let named = roll
        .iter()
        .find(|one| !one.current)
        .expect("the other one")
        .id;

    let ended = sessions
        .revoke_one(account, named)
        .await
        .expect("it revokes");

    assert!(ended);
    assert_eq!(sessions.resolve(&doomed).await, Err(SessionError::Unknown));
    assert!(sessions.resolve(&here).await.is_ok());
    assert!(
        !sessions
            .revoke_one(account, named)
            .await
            .expect("a second pass"),
        "a session is revoked once"
    );
}

#[tokio::test]
async fn an_account_cannot_revoke_a_session_it_does_not_keep() {
    let Some(pool) = control_plane("auth_revoke_theirs").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let mine = an_account(&pool, "person@example.com").await;
    let theirs = an_account(&pool, "other@example.com").await;
    let sessions = backend::sessions(&pg);
    let hers = sessions.issue(theirs, None, None).await.expect("a session");
    let current_one = sessions.issue(theirs, None, None).await.expect("another");
    let named = sessions
        .live(theirs, &current_one)
        .await
        .expect("a roll")
        .iter()
        .find(|one| !one.current)
        .expect("the other one")
        .id;

    let ended = sessions.revoke_one(mine, named).await.expect("it answers");

    assert!(!ended, "an id alone is not authority over a row");
    assert!(sessions.resolve(&hers).await.is_ok());
    assert!(
        !sessions
            .revoke_one(mine, SessionId::new(Uuid::new_v4()))
            .await
            .expect("it answers"),
        "an unknown id revokes nothing"
    );
}

#[tokio::test]
async fn signing_out_the_others_spares_the_session_that_asked() {
    let Some(pool) = control_plane("auth_revoke_others").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com").await;
    let theirs = an_account(&pool, "other@example.com").await;
    let sessions = backend::sessions(&pg);
    let here = sessions
        .issue(account, None, None)
        .await
        .expect("a session");
    let first = sessions.issue(account, None, None).await.expect("another");
    let second = sessions.issue(account, None, None).await.expect("a third");
    let untouched = sessions.issue(theirs, None, None).await.expect("not mine");

    let ended = sessions
        .revoke_others(account, &here)
        .await
        .expect("it revokes");

    assert_eq!(ended, 2);
    assert!(sessions.resolve(&here).await.is_ok());
    assert_eq!(sessions.resolve(&first).await, Err(SessionError::Unknown));
    assert_eq!(sessions.resolve(&second).await, Err(SessionError::Unknown));
    assert!(sessions.resolve(&untouched).await.is_ok());
}
