use backend::Each;
use ferristrace_application::registry::domain;
use ferristrace_domain::{
    AccountId, Counted, Decision, Fingerprint, Issue, IssueDecisions, ProjectId, Status,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

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

async fn an_account(pool: &PgPool, email: &str) -> AccountId {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await
        .expect("inserting an account");
    AccountId::new(id)
}

async fn rows(pool: &PgPool) -> i64 {
    sqlx::query("SELECT count(*) AS value FROM issue_decisions")
        .fetch_one(pool)
        .await
        .expect("counting the decisions")
        .get("value")
}

fn project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

fn a_print() -> Fingerprint {
    Fingerprint::of("checkout", "TimeoutError", "deadline exceeded after 960ms")
}

fn seen_at(last_seen_unix_nanos: u64) -> Counted {
    Counted {
        project: project(10),
        print: a_print(),
        exception_type: "TimeoutError".into(),
        message: "deadline exceeded after 960ms".into(),
        service: "checkout".into(),
        count: 4,
        first_seen_unix_nanos: 1,
        last_seen_unix_nanos,
    }
}

async fn the_decision(
    decisions: &Each<'_, domain::Decisions, FixedClock>,
    project: ProjectId,
) -> Decision {
    let found = decisions
        .read_many(&[(project, a_print())])
        .await
        .expect("the decisions are read");
    assert_eq!(found.len(), 1, "exactly one decision for the pair");
    assert_eq!((found[0].project, found[0].print), (project, a_print()));
    found[0].decision
}

#[tokio::test]
async fn an_issue_nobody_touched_has_no_row() {
    let Some(pool) = control_plane("decisions_untouched").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let decisions = backend::decisions(&pg);

    let found = decisions
        .read_many(&[(project(10), a_print())])
        .await
        .expect("the decisions are read");

    assert!(found.is_empty(), "nothing is written to say unresolved");
    assert_eq!(rows(&pool).await, 0);
    assert_eq!(
        Issue::of(seen_at(5), Decision::default()).decision.status,
        Status::Unresolved
    );
}

#[tokio::test]
async fn resolving_writes_the_status_the_moment_and_the_account() {
    let Some(pool) = control_plane("decisions_resolve").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let decisions = backend::decisions(&pg);
    let alice = an_account(&pool, "alice@acme.test").await;

    decisions
        .decide(project(10), a_print(), Status::Resolved, alice, 20)
        .await
        .expect("the issue is resolved");

    assert_eq!(
        the_decision(&decisions, project(10)).await,
        Decision {
            status: Status::Resolved,
            owner: None,
            decided_at_unix_nanos: 20,
            decided_by: Some(alice),
        }
    );
    assert_eq!(rows(&pool).await, 1);
}

#[tokio::test]
async fn deciding_again_replaces_the_status_and_keeps_the_owner() {
    let Some(pool) = control_plane("decisions_again").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let decisions = backend::decisions(&pg);
    let alice = an_account(&pool, "alice@acme.test").await;
    let bob = an_account(&pool, "bob@acme.test").await;

    decisions
        .assign(project(10), a_print(), Some(bob), alice, 10)
        .await
        .expect("bob owns the issue");
    decisions
        .decide(project(10), a_print(), Status::Resolved, alice, 20)
        .await
        .expect("alice resolves it");
    decisions
        .decide(project(10), a_print(), Status::Ignored, bob, 30)
        .await
        .expect("bob ignores it");

    assert_eq!(
        the_decision(&decisions, project(10)).await,
        Decision {
            status: Status::Ignored,
            owner: Some(bob),
            decided_at_unix_nanos: 30,
            decided_by: Some(bob),
        }
    );
    assert_eq!(
        rows(&pool).await,
        1,
        "one row per issue, whatever happens to it"
    );
}

#[tokio::test]
async fn assigning_an_owner_keeps_the_moment_a_status_was_decided() {
    let Some(pool) = control_plane("decisions_assign").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let decisions = backend::decisions(&pg);
    let alice = an_account(&pool, "alice@acme.test").await;
    let bob = an_account(&pool, "bob@acme.test").await;

    decisions
        .decide(project(10), a_print(), Status::Resolved, alice, 20)
        .await
        .expect("alice resolves it");
    decisions
        .assign(project(10), a_print(), Some(bob), alice, 50)
        .await
        .expect("alice hands it to bob");

    let decision = the_decision(&decisions, project(10)).await;
    assert_eq!(
        decision,
        Decision {
            status: Status::Resolved,
            owner: Some(bob),
            decided_at_unix_nanos: 20,
            decided_by: Some(alice),
        }
    );
    assert!(
        Issue::of(seen_at(40), decision).regressed,
        "an occurrence after the resolution still reads as a regression once reassigned"
    );
    assert!(
        !Issue::of(seen_at(15), decision).regressed,
        "an occurrence before the resolution is not one"
    );

    decisions
        .assign(project(10), a_print(), None, bob, 60)
        .await
        .expect("bob lets go of it");
    let released = the_decision(&decisions, project(10)).await;
    assert_eq!(released.owner, None);
    assert_eq!(released.decided_at_unix_nanos, 20);
    assert_eq!(released.status, Status::Resolved);
}

#[tokio::test]
async fn assigning_an_untouched_issue_leaves_it_unresolved() {
    let Some(pool) = control_plane("decisions_assign_first").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let decisions = backend::decisions(&pg);
    let alice = an_account(&pool, "alice@acme.test").await;
    let bob = an_account(&pool, "bob@acme.test").await;

    decisions
        .assign(project(10), a_print(), Some(bob), alice, 10)
        .await
        .expect("bob owns the issue");

    let decision = the_decision(&decisions, project(10)).await;
    assert_eq!(decision.status, Status::Unresolved);
    assert_eq!(decision.owner, Some(bob));
}

#[tokio::test]
async fn a_decision_is_keyed_by_project_and_print() {
    let Some(pool) = control_plane("decisions_keyed").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let decisions = backend::decisions(&pg);
    let alice = an_account(&pool, "alice@acme.test").await;
    let other = Fingerprint::of("payments", "DbError", "connection refused");

    decisions
        .decide(project(10), a_print(), Status::Resolved, alice, 20)
        .await
        .expect("resolved in the first project");
    decisions
        .decide(project(11), a_print(), Status::Ignored, alice, 21)
        .await
        .expect("ignored in the second project");
    decisions
        .decide(project(10), other, Status::Ignored, alice, 22)
        .await
        .expect("another issue of the first project");

    assert_eq!(
        the_decision(&decisions, project(10)).await.status,
        Status::Resolved
    );
    assert_eq!(
        the_decision(&decisions, project(11)).await.status,
        Status::Ignored
    );

    let both = decisions
        .read_many(&[(project(10), a_print()), (project(11), a_print())])
        .await
        .expect("the decisions are read");
    assert_eq!(both.len(), 2, "one print in two projects is two decisions");

    let none = decisions
        .read_many(&[])
        .await
        .expect("an empty question has an empty answer");
    assert!(none.is_empty());

    let unknown = decisions
        .read_many(&[(project(12), a_print()), (project(11), other)])
        .await
        .expect("the decisions are read");
    assert!(
        unknown.is_empty(),
        "a project or print never decided has no row"
    );
}

#[tokio::test]
async fn a_deleted_account_leaves_the_decision_in_place() {
    let Some(pool) = control_plane("decisions_outlive_accounts").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let decisions = backend::decisions(&pg);
    let alice = an_account(&pool, "alice@acme.test").await;

    decisions
        .decide(project(10), a_print(), Status::Resolved, alice, 20)
        .await
        .expect("alice resolves it");
    decisions
        .assign(project(10), a_print(), Some(alice), alice, 30)
        .await
        .expect("alice owns it");
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(alice.as_uuid())
        .execute(&pool)
        .await
        .expect("alice leaves");

    assert_eq!(
        the_decision(&decisions, project(10)).await,
        Decision {
            status: Status::Resolved,
            owner: None,
            decided_at_unix_nanos: 20,
            decided_by: None,
        },
        "the status and its moment outlive the account that wrote them"
    );
}
