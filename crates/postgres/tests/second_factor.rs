use std::time::Duration;

use ferristrace_domain::{
    AccountId, ChallengeToken, FactorError, FactorKind, FactorStore, Gate, RecoveryCode,
    SECRET_BYTES, SecretHash, Step, TotpSecret,
};
use ferristrace_infra::{FixedClock, SecretSeal};
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;
const APP: FactorKind = FactorKind::Authenticator;

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

fn factors(pool: &PgPool, at: u64) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(at)).with_seal(Some(seal()))
}

fn unsealed(pool: &PgPool) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(NOW)).with_seal(None)
}

fn seal() -> SecretSeal {
    SecretSeal::from_hex(&hex::encode([6_u8; 32])).expect("a key")
}

fn a_secret() -> TotpSecret {
    TotpSecret::from_bytes([2_u8; SECRET_BYTES])
}

async fn row_secret(pool: &PgPool, account: AccountId) -> Vec<u8> {
    sqlx::query("SELECT secret AS secret FROM auth_factors WHERE account_id = $1")
        .bind(account.as_uuid())
        .fetch_one(pool)
        .await
        .expect("a row")
        .get("secret")
}

#[tokio::test]
async fn a_kept_secret_comes_back_and_the_row_holds_neither_form_of_it() {
    let Some(pool) = control_plane("factors_sealed").await else {
        return;
    };
    let account = an_account(&pool, "sealed@example.com").await;
    let store_pg = factors(&pool, NOW);
    let store = backend::factors(&store_pg);
    let secret = a_secret();

    store.keep(account, APP, &secret).await.expect("it is kept");

    let kept = store
        .kept(account, APP)
        .await
        .expect("a lookup")
        .expect("a factor");
    assert_eq!(kept.secret, secret);
    assert!(!kept.confirmed, "a fresh enrolment is not confirmed");
    assert_eq!(kept.last_step, None);
    let row = row_secret(&pool, account).await;
    assert!(
        !row.windows(SECRET_BYTES)
            .any(|window| window == secret.as_bytes().as_slice()),
        "the clear bytes are not in the row"
    );
    assert!(!String::from_utf8_lossy(&row).contains(&secret.base32()));
}

#[tokio::test]
async fn an_unconfirmed_factor_is_not_offered_and_a_confirmed_one_is() {
    let Some(pool) = control_plane("factors_offered").await else {
        return;
    };
    let account = an_account(&pool, "offered@example.com").await;
    let store_pg = factors(&pool, NOW);
    let store = backend::factors(&store_pg);
    store.keep(account, APP, &a_secret()).await.expect("kept");

    assert_eq!(store.live(account).await.expect("a lookup"), Vec::new());
    assert_eq!(store.offered(account).await.expect("a lookup"), Vec::new());

    store.confirm(account, APP).await.expect("it confirms");

    assert_eq!(store.live(account).await.expect("a lookup"), vec![APP]);
    assert_eq!(
        store.confirm(account, APP).await,
        Err(FactorError::NoFactor),
        "a confirmed factor is not confirmed twice"
    );
}

#[tokio::test]
async fn a_confirmed_factor_is_not_quietly_replaced_by_a_second_enrolment() {
    let Some(pool) = control_plane("factors_replace").await else {
        return;
    };
    let account = an_account(&pool, "replace@example.com").await;
    let store_pg = factors(&pool, NOW);
    let store = backend::factors(&store_pg);
    let first = a_secret();
    store.keep(account, APP, &first).await.expect("kept");
    let other = TotpSecret::from_bytes([9_u8; SECRET_BYTES]);

    store
        .keep(account, APP, &other)
        .await
        .expect("an unconfirmed enrolment is replaced");
    let kept = store
        .kept(account, APP)
        .await
        .expect("a lookup")
        .expect("a factor");
    assert_eq!(kept.secret, other);

    store.confirm(account, APP).await.expect("it confirms");

    assert_eq!(
        store.keep(account, APP, &first).await,
        Err(FactorError::AlreadyKept)
    );
}

#[tokio::test]
async fn the_last_spent_step_is_remembered() {
    let Some(pool) = control_plane("factors_step").await else {
        return;
    };
    let account = an_account(&pool, "step@example.com").await;
    let store_pg = factors(&pool, NOW);
    let store = backend::factors(&store_pg);
    store.keep(account, APP, &a_secret()).await.expect("kept");

    store
        .spent_step(account, APP, Step::new(42))
        .await
        .expect("a step");

    let kept = store
        .kept(account, APP)
        .await
        .expect("a lookup")
        .expect("a factor");
    assert_eq!(kept.last_step, Some(Step::new(42)));
}

#[tokio::test]
async fn forgetting_says_whether_anything_was_there() {
    let Some(pool) = control_plane("factors_forget").await else {
        return;
    };
    let account = an_account(&pool, "forget@example.com").await;
    let store_pg = factors(&pool, NOW);
    let store = backend::factors(&store_pg);
    store.keep(account, APP, &a_secret()).await.expect("kept");

    assert!(store.forget(account, APP).await.expect("a delete"));
    assert!(!store.forget(account, APP).await.expect("a delete"));
    assert_eq!(store.kept(account, APP).await.expect("a lookup"), None);
}

#[tokio::test]
async fn recovery_codes_are_kept_spent_once_and_replaced_whole() {
    let Some(pool) = control_plane("factors_recovery").await else {
        return;
    };
    let account = an_account(&pool, "recovery@example.com").await;
    let store_pg = factors(&pool, NOW);
    let store = backend::factors(&store_pg);
    let first = RecoveryCode::ten();
    let digests: Vec<SecretHash> = first.iter().map(RecoveryCode::hash).collect();

    store
        .keep_recovery(account, &digests)
        .await
        .expect("they are kept");

    assert_eq!(store.recovery_left(account).await.expect("a count"), 10);
    assert!(
        store
            .spend_recovery(account, first[0].hash())
            .await
            .expect("a spend")
    );
    assert!(
        !store
            .spend_recovery(account, first[0].hash())
            .await
            .expect("a spend"),
        "a code is spent once"
    );
    assert_eq!(store.recovery_left(account).await.expect("a count"), 9);

    let second = RecoveryCode::ten();
    let fresh: Vec<SecretHash> = second.iter().map(RecoveryCode::hash).collect();
    store
        .keep_recovery(account, &fresh)
        .await
        .expect("they are replaced");

    assert_eq!(store.recovery_left(account).await.expect("a count"), 10);
    assert!(
        !store
            .spend_recovery(account, first[1].hash())
            .await
            .expect("a spend"),
        "a code from the old set opens nothing"
    );
    store
        .keep_recovery(account, &[])
        .await
        .expect("they are dropped");
    assert_eq!(store.recovery_left(account).await.expect("a count"), 0);
}

#[tokio::test]
async fn a_recovery_code_belongs_to_the_account_that_was_given_it() {
    let Some(pool) = control_plane("factors_recovery_owner").await else {
        return;
    };
    let mine = an_account(&pool, "mine@example.com").await;
    let theirs = an_account(&pool, "theirs@example.com").await;
    let store_pg = factors(&pool, NOW);
    let store = backend::factors(&store_pg);
    let codes = RecoveryCode::ten();
    let digests: Vec<SecretHash> = codes.iter().map(RecoveryCode::hash).collect();
    store.keep_recovery(mine, &digests).await.expect("kept");

    assert!(
        !store
            .spend_recovery(theirs, codes[0].hash())
            .await
            .expect("a spend")
    );
    assert_eq!(store.recovery_left(mine).await.expect("a count"), 10);
}

#[tokio::test]
async fn a_challenge_counts_its_answers_and_is_closed_once() {
    let Some(pool) = control_plane("factors_challenge").await else {
        return;
    };
    let account = an_account(&pool, "challenge@example.com").await;
    let store_pg = factors(&pool, NOW);
    let store = backend::factors(&store_pg);
    let challenge = ChallengeToken::random();

    store
        .open(account, challenge.hash())
        .await
        .expect("it opens");

    let first = store.answered(challenge.hash()).await.expect("an answer");
    let second = store.answered(challenge.hash()).await.expect("an answer");
    assert_eq!(first.account, account);
    assert_eq!(first.answers, 1);
    assert_eq!(second.answers, 2);

    store.close(challenge.hash()).await.expect("it closes");

    assert_eq!(
        store.answered(challenge.hash()).await,
        Err(FactorError::NoChallenge)
    );
}

#[tokio::test]
async fn a_challenge_past_its_window_answers_nothing_and_prunes() {
    let Some(pool) = control_plane("factors_window").await else {
        return;
    };
    let account = an_account(&pool, "window@example.com").await;
    let opening_pg = factors(&pool, NOW).with_factor_window(Duration::from_secs(60));
    let opening = backend::factors(&opening_pg);
    let challenge = ChallengeToken::random();
    opening
        .open(account, challenge.hash())
        .await
        .expect("it opens");

    let later_pg = factors(&pool, NOW + 61);
    let later = backend::factors(&later_pg);

    assert_eq!(
        later.answered(challenge.hash()).await,
        Err(FactorError::NoChallenge)
    );
    assert_eq!(later.prune().await.expect("a prune"), 1);
    assert_eq!(later.prune().await.expect("a prune"), 0);
}

#[tokio::test]
async fn a_deployment_without_a_key_keeps_no_factor_but_still_reads_what_is_offered() {
    let Some(pool) = control_plane("factors_unsealed").await else {
        return;
    };
    let account = an_account(&pool, "unsealed@example.com").await;
    let sealed_pg = factors(&pool, NOW);
    let sealed = backend::factors(&sealed_pg);
    let plain_pg = unsealed(&pool);
    let plain = backend::factors(&plain_pg);

    assert_eq!(
        plain.keep(account, APP, &a_secret()).await,
        Err(FactorError::NotOffered)
    );
    assert_eq!(plain.live(account).await.expect("a lookup"), Vec::new());

    sealed.keep(account, APP, &a_secret()).await.expect("kept");
    sealed.confirm(account, APP).await.expect("confirmed");

    assert_eq!(
        plain.live(account).await.expect("a lookup"),
        vec![APP],
        "what is offered is readable without the key"
    );
    assert_eq!(
        plain.kept(account, APP).await,
        Err(FactorError::NotOffered),
        "but the secret behind it is not"
    );
}

#[tokio::test]
async fn a_factor_leaves_with_the_account_that_kept_it() {
    let Some(pool) = control_plane("factors_cascade").await else {
        return;
    };
    let account = an_account(&pool, "cascade@example.com").await;
    let store_pg = factors(&pool, NOW);
    let store = backend::factors(&store_pg);
    store.keep(account, APP, &a_secret()).await.expect("kept");
    let digests: Vec<SecretHash> = RecoveryCode::ten().iter().map(RecoveryCode::hash).collect();
    store.keep_recovery(account, &digests).await.expect("kept");
    store
        .open(account, ChallengeToken::random().hash())
        .await
        .expect("it opens");

    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(account.as_uuid())
        .execute(&pool)
        .await
        .expect("the account goes");

    assert_eq!(store.kept(account, APP).await.expect("a lookup"), None);
    assert_eq!(store.recovery_left(account).await.expect("a count"), 0);
}
