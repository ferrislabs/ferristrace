use ferristrace_domain::{
    AccountId, AccountName, Accounts, Agent, AuthError, AuthProvider, AuthSources, CodeVerifier,
    Email, FlowError, Issuer, Mailer, NoProvider, NoSecondFactor, OidcFlows, Opened, Password,
    PasswordCheck, PasswordHasher, PendingFlow, PhotoKind, Portrait, ProvisionPolicy, ResetError,
    ResetPassword, ResetStore, ResetToken, ResolvedIdentity, SessionError, SessionId,
    SessionLifetime, SessionRoll, SessionStore, SignIn, SignInProof, Subject,
};
use ferristrace_infra::{Argon2Hasher, FixedClock};
use ferristrace_postgres::{AccountError, CONTROL_PLANE, Postgres};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;

fn cheap() -> Argon2Hasher {
    Argon2Hasher::with_cost(8, 1, 1).expect("a cheap hasher")
}

fn dearer() -> Argon2Hasher {
    Argon2Hasher::with_cost(32, 2, 1).expect("a dearer hasher")
}

fn lifetime() -> SessionLifetime {
    SessionLifetime {
        idle_seconds: 100,
        hard_seconds: 1000,
    }
}

fn at(pool: &PgPool, instant: u64) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(instant))
        .with_session_lifetime(lifetime())
        .with_hasher(cheap())
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

async fn an_account(pool: &PgPool, email: &str, password: Option<&str>) -> AccountId {
    let id = Uuid::new_v4();
    let hash = match password {
        Some(plaintext) => Some(
            cheap()
                .hash(&Password::new(plaintext))
                .await
                .expect("a hash")
                .encoded()
                .to_owned(),
        ),
        None => None,
    };
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(email)
        .bind(hash)
        .execute(pool)
        .await
        .expect("an account");
    AccountId::new(id)
}

fn a_proof(email: &str, password: &str) -> SignInProof {
    SignInProof::Password {
        email: Email::try_from(email).expect("an address"),
        password: Password::new(password),
    }
}

#[tokio::test]
async fn a_hash_accepts_its_own_password() {
    let hasher = cheap();
    let password = Password::new("correct horse battery");

    let hash = hasher.hash(&password).await.expect("a hash");
    let check = hasher.check(&password, &hash).await.expect("a check");

    assert_eq!(check, PasswordCheck::Accepted);
    assert!(
        hash.encoded().starts_with("$argon2id$"),
        "{}",
        hash.encoded()
    );
}

#[tokio::test]
async fn a_wrong_password_is_rejected() {
    let hasher = cheap();
    let hash = hasher
        .hash(&Password::new("correct horse battery"))
        .await
        .expect("a hash");

    let check = hasher
        .check(&Password::new("wrong horse battery"), &hash)
        .await
        .expect("a check");

    assert_eq!(check, PasswordCheck::Rejected);
}

#[tokio::test]
async fn the_decoy_matches_nothing() {
    let hasher = cheap();

    let check = hasher
        .check(&Password::new("anything at all"), hasher.decoy())
        .await
        .expect("a check");

    assert_eq!(check, PasswordCheck::Rejected);
}

#[tokio::test]
async fn a_hash_below_the_current_cost_is_rewritten() {
    let password = Password::new("correct horse battery");
    let old = cheap().hash(&password).await.expect("a cheap hash");

    let check = dearer().check(&password, &old).await.expect("a check");

    match check {
        PasswordCheck::AcceptedBehindCost(upgraded) => {
            assert_ne!(upgraded.encoded(), old.encoded());
        }
        other => panic!("the stale cost went unnoticed: {other:?}"),
    }
}

#[tokio::test]
async fn a_session_resolves_to_its_account() {
    let Some(pool) = control_plane("auth_session").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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

#[tokio::test]
async fn a_correct_password_resolves_a_local_identity() {
    let Some(pool) = control_plane("auth_local_ok").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com", Some("correct horse")).await;
    let provider = backend::local(&pg);

    let identity = provider
        .sign_in(a_proof("person@example.com", "correct horse"))
        .await
        .expect("the password matches");

    assert!(identity.issuer.is_local());
    assert_eq!(identity.subject.as_str(), account.as_uuid().to_string());
    assert_eq!(identity.email.as_str(), "person@example.com");
}

#[tokio::test]
async fn a_wrong_password_an_unknown_address_and_no_password_all_refuse() {
    let Some(pool) = control_plane("auth_local_refused").await else {
        return;
    };
    let pg = at(&pool, NOW);
    an_account(&pool, "person@example.com", Some("correct horse")).await;
    an_account(&pool, "federated@example.com", None).await;
    let provider = backend::local(&pg);

    for proof in [
        a_proof("person@example.com", "wrong horse"),
        a_proof("stranger@example.com", "correct horse"),
        a_proof("federated@example.com", "correct horse"),
    ] {
        assert_eq!(provider.sign_in(proof).await, Err(AuthError::Rejected));
    }
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

#[derive(Default)]
struct Outbox {
    sent: Mutex<Vec<String>>,
}

impl Outbox {
    fn last(&self) -> Option<String> {
        self.sent.lock().expect("the outbox lock").last().cloned()
    }

    fn count(&self) -> usize {
        self.sent.lock().expect("the outbox lock").len()
    }
}

impl Mailer for &Outbox {
    async fn send_password_reset(&self, _to: &Email, token: &ResetToken) -> Result<(), ResetError> {
        self.sent
            .lock()
            .expect("the outbox lock")
            .push(token.exposed().to_owned());
        Ok(())
    }
}

fn a_reset_token(seed: &str) -> ResetToken {
    ResetToken::from_plaintext(&format!("ct_pr_{seed}"))
}

#[tokio::test]
async fn a_reset_is_offered_only_to_an_address_we_know() {
    let Some(pool) = control_plane("reset_known").await else {
        return;
    };
    let pg = at(&pool, NOW);
    an_account(&pool, "person@example.com", Some("old secret")).await;
    let outbox = Outbox::default();
    let reset = ResetPassword::new(backend::resets(&pg), cheap(), &outbox);

    let known = reset
        .request(
            &Email::try_from("person@example.com").expect("an address"),
            a_reset_token("one"),
        )
        .await
        .expect("the request runs");
    let unknown = reset
        .request(
            &Email::try_from("stranger@example.com").expect("an address"),
            a_reset_token("two"),
        )
        .await
        .expect("the request runs");

    assert!(known);
    assert!(!unknown);
    assert_eq!(
        outbox.count(),
        1,
        "no message leaves for an unknown address"
    );
}

#[tokio::test]
async fn a_confirmed_reset_changes_the_password_and_ends_every_session() {
    let Some(pool) = control_plane("reset_confirm").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com", Some("old secret")).await;
    let sessions = backend::sessions(&pg);
    let token = sessions
        .issue(account, None, None)
        .await
        .expect("a session");
    let outbox = Outbox::default();
    let reset = ResetPassword::new(backend::resets(&pg), cheap(), &outbox);
    let address = Email::try_from("person@example.com").expect("an address");
    reset
        .request(&address, a_reset_token("one"))
        .await
        .expect("the request");
    let sent = ResetToken::from_plaintext(&outbox.last().expect("a message"));

    let ended = reset
        .confirm(&sent, &Password::new("a new secret"))
        .await
        .expect("the confirmation");

    assert_eq!(ended, 1, "the session it was signed in with is gone");
    assert_eq!(sessions.resolve(&token).await, Err(SessionError::Unknown));

    let provider = backend::local(&pg);
    assert!(
        provider
            .sign_in(a_proof("person@example.com", "a new secret"))
            .await
            .is_ok(),
        "the new password works"
    );
    assert_eq!(
        provider
            .sign_in(a_proof("person@example.com", "old secret"))
            .await,
        Err(AuthError::Rejected),
        "the old one does not"
    );
}

#[tokio::test]
async fn a_reset_token_is_spent_by_its_first_use() {
    let Some(pool) = control_plane("reset_once").await else {
        return;
    };
    let pg = at(&pool, NOW);
    an_account(&pool, "person@example.com", Some("old secret")).await;
    let outbox = Outbox::default();
    let reset = ResetPassword::new(backend::resets(&pg), cheap(), &outbox);
    let token = a_reset_token("one");
    reset
        .request(
            &Email::try_from("person@example.com").expect("an address"),
            token.clone(),
        )
        .await
        .expect("the request");

    reset
        .confirm(&token, &Password::new("a new secret"))
        .await
        .expect("the first use");
    let replay = reset
        .confirm(&token, &Password::new("another good one"))
        .await;

    assert_eq!(replay, Err(ResetError::Unknown));
}

#[tokio::test]
async fn a_reset_past_its_window_is_gone() {
    let Some(pool) = control_plane("reset_expired").await else {
        return;
    };
    an_account(&pool, "person@example.com", Some("old secret")).await;
    let outbox = Outbox::default();
    let token = a_reset_token("one");
    ResetPassword::new(
        backend::resets(&at(&pool, NOW).with_reset_window(Duration::from_secs(60))),
        cheap(),
        &outbox,
    )
    .request(
        &Email::try_from("person@example.com").expect("an address"),
        token.clone(),
    )
    .await
    .expect("the request");

    let late = ResetPassword::new(
        backend::resets(&at(&pool, NOW + 61).with_reset_window(Duration::from_secs(60))),
        cheap(),
        &outbox,
    )
    .confirm(&token, &Password::new("a new secret"))
    .await;

    assert_eq!(late, Err(ResetError::Unknown));
}

#[tokio::test]
async fn a_created_account_is_locked_until_a_password_is_chosen() {
    let Some(pool) = control_plane("accounts_locked").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let address = Email::try_from("fresh@example.com").expect("an address");
    let locked = cheap().locked().expect("a locked hash");
    let account = backend::accounts(&pg)
        .create(&address, &locked)
        .await
        .expect("the account is created");
    let provider = backend::local(&pg);

    assert_eq!(
        provider.sign_in(a_proof("fresh@example.com", "")).await,
        Err(AuthError::Rejected),
        "an empty password must not open a locked account"
    );
    assert_eq!(
        provider
            .sign_in(a_proof("fresh@example.com", "correct horse"))
            .await,
        Err(AuthError::Rejected)
    );

    let outbox = Outbox::default();
    let resets = backend::resets(&pg);
    let invite = a_reset_token("invite");
    resets
        .begin(account, invite.hash())
        .await
        .expect("the invite is opened");
    ResetPassword::new(resets, cheap(), &outbox)
        .confirm(&invite, &Password::new("chosen at last"))
        .await
        .expect("the password is chosen");

    assert!(
        provider
            .sign_in(a_proof("fresh@example.com", "chosen at last"))
            .await
            .is_ok(),
        "the account opens once its password is chosen"
    );
}

#[tokio::test]
async fn an_address_carries_at_most_one_account() {
    let Some(pool) = control_plane("accounts_taken").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let address = Email::try_from("twice@example.com").expect("an address");
    let accounts = backend::accounts(&pg);
    let locked = cheap().locked().expect("a locked hash");
    accounts
        .create(&address, &locked)
        .await
        .expect("the first account");

    let refused = accounts.create(&address, &locked).await;

    assert!(matches!(refused, Err(AccountError::Taken)));
}

#[tokio::test]
async fn two_locked_accounts_do_not_share_a_hash() {
    let hasher = cheap();
    let one = hasher.locked().expect("a locked hash");
    let other = hasher.locked().expect("another locked hash");

    assert_ne!(one.encoded(), other.encoded());
}

async fn accounts_held(pool: &PgPool) -> i64 {
    sqlx::query("SELECT count(*) AS value FROM accounts")
        .fetch_one(pool)
        .await
        .expect("counting the accounts")
        .get("value")
}

#[tokio::test]
async fn a_local_password_opens_a_session_without_provisioning_anything() {
    let Some(pool) = control_plane("auth_local_end_to_end").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com", Some("correct horse")).await;
    let sessions = Arc::new(backend::sessions(&pg));
    let sign_in = SignIn::new(
        AuthSources::new(Some(backend::local(&pg)), None::<NoProvider>),
        backend::accounts(&pg),
        Arc::clone(&sessions),
        NoSecondFactor,
        ProvisionPolicy::default(),
    );

    let admitted = sign_in
        .begin(a_proof("person@example.com", "correct horse"), None)
        .await
        .expect("the password opens a session");

    let Opened::Session(token) = admitted else {
        panic!("an account with no factor is admitted at once");
    };
    let session = sessions.resolve(&token).await.expect("the session is live");
    assert_eq!(session.account, account);
    assert_eq!(
        accounts_held(&pool).await,
        1,
        "signing in must not provision a second account for an address we hold"
    );
}

#[tokio::test]
async fn a_wrong_local_password_opens_nothing_and_provisions_nothing() {
    let Some(pool) = control_plane("auth_local_refused_end_to_end").await else {
        return;
    };
    let pg = at(&pool, NOW);
    an_account(&pool, "person@example.com", Some("correct horse")).await;
    let sign_in = SignIn::new(
        AuthSources::new(Some(backend::local(&pg)), None::<NoProvider>),
        backend::accounts(&pg),
        Arc::new(backend::sessions(&pg)),
        NoSecondFactor,
        ProvisionPolicy::default(),
    );

    let refused = sign_in
        .begin(a_proof("person@example.com", "wrong horse battery"), None)
        .await;

    assert!(refused.is_err());
    assert_eq!(accounts_held(&pool).await, 1);
}

#[tokio::test]
async fn a_local_identity_naming_no_account_is_not_provisioned() {
    let Some(pool) = control_plane("auth_local_stranger").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let identity = ResolvedIdentity {
        issuer: Issuer::Local,
        subject: Subject::try_from(Uuid::new_v4().to_string().as_str()).expect("a subject"),
        email: Email::try_from("ghost@example.com").expect("an address"),
        email_verified: true,
    };

    let refused = backend::accounts(&pg)
        .provision(&identity, ProvisionPolicy::default())
        .await;

    assert!(matches!(refused, Err(AccountError::NotProvisioned)));
    assert_eq!(
        accounts_held(&pool).await,
        0,
        "a local identity is never a reason to create an account"
    );
}

#[tokio::test]
async fn an_account_carries_no_name_until_somebody_writes_one() {
    let Some(pool) = control_plane("auth_unnamed").await else {
        return;
    };
    let pg = at(&pool, NOW);
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;

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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;

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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
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
    let mine = an_account(&pool, "person@example.com", Some("secret")).await;
    let theirs = an_account(&pool, "other@example.com", Some("secret")).await;
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
    let account = an_account(&pool, "person@example.com", Some("secret")).await;
    let theirs = an_account(&pool, "other@example.com", Some("secret")).await;
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
