use ferristrace_domain::{
    AccountId, Creation, ExplorerViews, NewView, OrgRole, Saving, TenantId, ViewChange,
    ViewDefinition, ViewName, ViewSignal, Visibility,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
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

async fn an_org(pool: &PgPool, owner: &str) -> TenantId {
    an_account(pool, owner).await;
    backend::memberships(&Postgres::new(pool.clone(), FixedClock::at(1_700_000_000)))
        .found("Acme", owner)
        .await
        .expect("the organisation is founded")
}

async fn a_member(pool: &PgPool, org: TenantId, email: &str) -> AccountId {
    let account = an_account(pool, email).await;
    backend::memberships(&Postgres::new(pool.clone(), FixedClock::at(1_700_000_000)))
        .grant(org, email, OrgRole::Member)
        .await
        .expect("the grant");
    account
}

fn new_view(name: &str, visibility: Visibility) -> NewView {
    NewView {
        signal: ViewSignal::Traces,
        name: ViewName::try_from(name).expect("a name"),
        visibility,
        definition: ViewDefinition::try_from(r#"{"q":[{"op":"eq"}],"order":"started"}"#)
            .expect("a definition"),
    }
}

fn changing(name: &str, visibility: Option<Visibility>) -> ViewChange {
    ViewChange {
        name: ViewName::try_from(name).expect("a name"),
        definition: ViewDefinition::try_from(r#"{"q":[]}"#).expect("a definition"),
        visibility,
    }
}

#[tokio::test]
async fn a_view_is_kept_with_its_definition_and_read_back_by_its_owner() {
    let Some(pool) = control_plane("views_roundtrip").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let alice = a_member(&pool, org, "alice@example.com").await;
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let views = backend::views(&pg);

    let Creation::Made(made) = views
        .create(org, alice, &new_view("Slow checkout", Visibility::Org))
        .await
        .expect("created")
    else {
        panic!("the name is free");
    };
    let found = views.find(org, made.id).await.expect("read").expect("held");

    assert_eq!(found, made);
    assert_eq!(&*found.name, "Slow checkout");
    assert_eq!(
        &*found.definition,
        r#"{"q": [{"op": "eq"}], "order": "started"}"#
    );
    assert_eq!(found.created_by, Some(alice));
    assert!(found.created_at_unix_nanos > 0);
}

#[tokio::test]
async fn private_views_are_listed_to_their_creator_only_and_organisation_views_to_everyone() {
    let Some(pool) = control_plane("views_visibility").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let alice = a_member(&pool, org, "alice@example.com").await;
    let bob = a_member(&pool, org, "bob@example.com").await;
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let views = backend::views(&pg);
    views
        .create(org, alice, &new_view("shared", Visibility::Org))
        .await
        .expect("created");
    views
        .create(org, alice, &new_view("alice only", Visibility::Private))
        .await
        .expect("created");
    views
        .create(org, bob, &new_view("bob only", Visibility::Private))
        .await
        .expect("created");

    let names = |seen: Vec<ferristrace_domain::View>| -> Vec<String> {
        seen.into_iter()
            .map(|view| view.name.into_string())
            .collect()
    };
    let alices = names(
        views
            .visible(org, ViewSignal::Traces, alice)
            .await
            .expect("read"),
    );
    let bobs = names(
        views
            .visible(org, ViewSignal::Traces, bob)
            .await
            .expect("read"),
    );

    assert_eq!(alices, vec!["alice only", "shared"]);
    assert_eq!(bobs, vec!["bob only", "shared"]);
}

#[tokio::test]
async fn names_are_unique_per_visibility_and_per_creator_for_private_views() {
    let Some(pool) = control_plane("views_names").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let alice = a_member(&pool, org, "alice@example.com").await;
    let bob = a_member(&pool, org, "bob@example.com").await;
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let views = backend::views(&pg);
    let made = |outcome: Creation| matches!(outcome, Creation::Made(_));

    let first = views
        .create(org, alice, &new_view("slow", Visibility::Org))
        .await
        .expect("created");
    let again = views
        .create(org, bob, &new_view("slow", Visibility::Org))
        .await
        .expect("created");
    let mine = views
        .create(org, alice, &new_view("slow", Visibility::Private))
        .await
        .expect("created");
    let his = views
        .create(org, bob, &new_view("slow", Visibility::Private))
        .await
        .expect("created");
    let twice = views
        .create(org, bob, &new_view("slow", Visibility::Private))
        .await
        .expect("created");

    assert!(made(first));
    assert_eq!(again, Creation::NameTaken);
    assert!(made(mine));
    assert!(made(his));
    assert_eq!(twice, Creation::NameTaken);
}

#[tokio::test]
async fn a_change_renames_rewrites_and_can_share_but_never_takes_a_taken_name() {
    let Some(pool) = control_plane("views_change").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let alice = a_member(&pool, org, "alice@example.com").await;
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let views = backend::views(&pg);
    views
        .create(org, alice, &new_view("taken", Visibility::Org))
        .await
        .expect("created");
    let Creation::Made(mine) = views
        .create(org, alice, &new_view("draft", Visibility::Private))
        .await
        .expect("created")
    else {
        panic!("free");
    };

    let clash = views
        .change(org, mine.id, &changing("taken", Some(Visibility::Org)))
        .await
        .expect("changed");
    let shared = views
        .change(org, mine.id, &changing("final", Some(Visibility::Org)))
        .await
        .expect("changed");
    let after = views.find(org, mine.id).await.expect("read").expect("held");
    let missing = views
        .change(
            org,
            ferristrace_domain::ViewId::new(Uuid::new_v4()),
            &changing("x", None),
        )
        .await
        .expect("changed");

    assert_eq!(clash, Saving::NameTaken);
    assert_eq!(shared, Saving::Saved);
    assert_eq!((&*after.name, after.visibility), ("final", Visibility::Org));
    assert_eq!(&*after.definition, r#"{"q": []}"#);
    assert!(after.updated_at_unix_nanos >= mine.updated_at_unix_nanos);
    assert_eq!(missing, Saving::Missing);
}

#[tokio::test]
async fn deleting_and_the_organisation_boundary() {
    let Some(pool) = control_plane("views_delete").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let other = an_org(&pool, "other@example.com").await;
    let alice = a_member(&pool, org, "alice@example.com").await;
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let views = backend::views(&pg);
    let Creation::Made(made) = views
        .create(org, alice, &new_view("slow", Visibility::Org))
        .await
        .expect("created")
    else {
        panic!("free");
    };

    let elsewhere = views.delete(other, made.id).await.expect("deleted");
    let found_elsewhere = views.find(other, made.id).await.expect("read");
    let here = views.delete(org, made.id).await.expect("deleted");
    let again = views.delete(org, made.id).await.expect("deleted");

    assert!(!elsewhere);
    assert_eq!(found_elsewhere, None);
    assert!(here);
    assert!(!again);
}

#[tokio::test]
async fn an_organisation_view_outlives_its_creator_account() {
    let Some(pool) = control_plane("views_orphan").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let alice = a_member(&pool, org, "alice@example.com").await;
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let views = backend::views(&pg);
    let Creation::Made(made) = views
        .create(org, alice, &new_view("shared", Visibility::Org))
        .await
        .expect("created")
    else {
        panic!("free");
    };

    sqlx::query("DELETE FROM org_members WHERE account_id = $1")
        .bind(alice.as_uuid())
        .execute(&pool)
        .await
        .expect("leaves");
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(alice.as_uuid())
        .execute(&pool)
        .await
        .expect("deleted");
    let kept = views
        .find(org, made.id)
        .await
        .expect("read")
        .expect("still there");

    assert_eq!(kept.created_by, None);
}
