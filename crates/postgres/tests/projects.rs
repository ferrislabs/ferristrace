use backend::Each;
use ferristrace_application::registry::domain;
use ferristrace_domain::{
    Assigned, Created, ProjectId, ProjectName, Projects, Renamed, Squads, StoreError, TeamId,
    TeamName, TenantId,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres, PostgresDirectory};
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

async fn an_org(pool: &PgPool, owner: &str) -> TenantId {
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'chosen')")
        .bind(Uuid::new_v4())
        .bind(owner)
        .execute(pool)
        .await
        .expect("inserting an account");
    backend::memberships(&Postgres::new(pool.clone(), FixedClock::at(1_700_000_000)))
        .found("Acme", owner)
        .await
        .expect("the organisation is founded")
}

fn a_slug(name: &str) -> ProjectName {
    ProjectName::try_from(name).expect("an admitted name")
}

async fn a_project(
    projects: &Each<'_, domain::Projects, FixedClock>,
    org: TenantId,
    name: &str,
) -> ProjectId {
    assert_eq!(
        projects.create(org, &a_slug(name)).await.expect("a create"),
        Created::Done
    );
    named(projects, org, name).await
}

async fn named(
    projects: &Each<'_, domain::Projects, FixedClock>,
    org: TenantId,
    name: &str,
) -> ProjectId {
    projects
        .of(org)
        .await
        .expect("the projects are read")
        .into_iter()
        .find(|project| &*project.name == name)
        .unwrap_or_else(|| panic!("{name} is in the catalogue"))
        .id
}

async fn a_team(pool: &PgPool, org: TenantId, name: &str) -> TeamId {
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let teams = backend::teams(&pg);
    teams
        .create(org, &TeamName::try_from(name).expect("a name"))
        .await
        .expect("the create");
    teams
        .of(org)
        .await
        .expect("the teams")
        .into_iter()
        .find(|team| &*team.name == name)
        .expect("the team is there")
        .id
}

async fn keys_live(pool: &PgPool, project: ProjectId) -> i64 {
    sqlx::query(
        "SELECT count(*) AS value FROM api_keys WHERE project_id = $1 AND revoked_at IS NULL",
    )
    .bind(project.as_uuid())
    .fetch_one(pool)
    .await
    .expect("counting the keys")
    .get("value")
}

#[tokio::test]
async fn a_created_project_reads_back_with_no_team_and_no_keys() {
    let Some(pool) = control_plane("projects_create").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let projects = backend::projects(&pg);

    projects
        .create(org, &a_slug("storefront"))
        .await
        .expect("the create");

    let held = projects.of(org).await.expect("the projects");
    assert_eq!(held.len(), 1);
    assert_eq!(&*held[0].name, "storefront");
    assert_eq!(held[0].team, None);
    assert_eq!(held[0].keys, 0);
    assert!(
        held[0].created_at_unix_nanos > 1_600_000_000_000_000_000,
        "the column holds nanoseconds, not seconds: {}",
        held[0].created_at_unix_nanos
    );
}

#[tokio::test]
async fn a_name_one_organisation_holds_is_still_free_in_another() {
    let Some(pool) = control_plane("projects_names").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let mine = an_org(&pool, "mine@example.com").await;
    let theirs = an_org(&pool, "theirs@example.com").await;
    let projects = backend::projects(&pg);
    a_project(&projects, mine, "storefront").await;

    let again = projects
        .create(mine, &a_slug("storefront"))
        .await
        .expect("the second create");
    let elsewhere = projects
        .create(theirs, &a_slug("storefront"))
        .await
        .expect("the create in another organisation");

    assert_eq!(again, Created::Taken);
    assert_eq!(elsewhere, Created::Done);
}

#[tokio::test]
async fn renaming_a_project_keeps_its_id_and_its_keys() {
    let Some(pool) = control_plane("projects_rename").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let projects = backend::projects(&pg);
    let storefront = a_project(&projects, org, "storefront").await;
    PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000))
        .issue(org, Some(storefront.as_uuid()))
        .await
        .expect("a key");

    let renamed = projects
        .rename(org, storefront, &a_slug("shopfront"))
        .await
        .expect("the rename");

    assert_eq!(renamed, Renamed::Done);
    assert_eq!(named(&projects, org, "shopfront").await, storefront);
    assert_eq!(keys_live(&pool, storefront).await, 1);
}

#[tokio::test]
async fn renaming_a_project_tells_a_clash_apart_from_a_project_that_is_not_there() {
    let Some(pool) = control_plane("projects_rename_refused").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let projects = backend::projects(&pg);
    let storefront = a_project(&projects, org, "storefront").await;
    a_project(&projects, org, "payments-eu").await;

    let clash = projects
        .rename(org, storefront, &a_slug("payments-eu"))
        .await
        .expect("the clashing rename");
    let gone = projects
        .rename(org, ProjectId::new(Uuid::new_v4()), &a_slug("whatever"))
        .await
        .expect("the rename of nothing");

    assert_eq!(clash, Renamed::Taken);
    assert_eq!(gone, Renamed::Gone);
    assert_eq!(named(&projects, org, "storefront").await, storefront);
}

#[tokio::test]
async fn the_count_of_keys_is_the_count_that_still_authenticate() {
    let Some(pool) = control_plane("projects_keys").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let projects = backend::projects(&pg);
    let storefront = a_project(&projects, org, "storefront").await;
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));
    let drawn = directory
        .issue(org, Some(storefront.as_uuid()))
        .await
        .expect("a key");
    directory
        .issue(org, Some(storefront.as_uuid()))
        .await
        .expect("another key");

    directory
        .revoke(drawn.split().expect("a prefix and a secret").0)
        .await
        .expect("the revocation");

    let held = projects.of(org).await.expect("the projects");
    assert_eq!(
        held[0].keys, 1,
        "a revoked key is not a way in, so it is not counted"
    );
}

#[tokio::test]
async fn a_project_is_handed_to_a_team_and_taken_back_again() {
    let Some(pool) = control_plane("projects_assign").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let projects = backend::projects(&pg);
    let storefront = a_project(&projects, org, "storefront").await;
    let backend = a_team(&pool, org, "Backend").await;

    assert_eq!(
        projects
            .assign(org, storefront, Some(backend))
            .await
            .expect("the hand over"),
        Assigned::Done
    );
    let owned = projects.of(org).await.expect("the projects");
    assert_eq!(
        projects
            .assign(org, storefront, None)
            .await
            .expect("taking it back"),
        Assigned::Done
    );

    let team = owned[0].team.as_ref().expect("a team holds it");
    assert_eq!(team.id, backend);
    assert_eq!(&*team.name, "Backend");
    assert_eq!(projects.of(org).await.expect("the projects")[0].team, None);
}

#[tokio::test]
async fn a_team_of_another_organisation_is_refused_the_project() {
    let Some(pool) = control_plane("projects_their_team").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let mine = an_org(&pool, "mine@example.com").await;
    let theirs = an_org(&pool, "theirs@example.com").await;
    let projects = backend::projects(&pg);
    let storefront = a_project(&projects, mine, "storefront").await;
    let theirteam = a_team(&pool, theirs, "Backend").await;

    let refused = projects
        .assign(mine, storefront, Some(theirteam))
        .await
        .expect("the control plane answers");

    assert_eq!(refused, Assigned::NoSuchTeam);
    assert_eq!(projects.of(mine).await.expect("the projects")[0].team, None);
}

#[tokio::test]
async fn assigning_a_project_the_organisation_does_not_hold_is_told_apart_from_a_bad_team() {
    let Some(pool) = control_plane("projects_assign_gone").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let projects = backend::projects(&pg);

    let gone = projects
        .assign(org, ProjectId::new(Uuid::new_v4()), None)
        .await
        .expect("the control plane answers");

    assert_eq!(gone, Assigned::Gone);
}

#[tokio::test]
async fn a_project_of_another_organisation_is_not_there_to_arrange() {
    let Some(pool) = control_plane("projects_theirs").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let mine = an_org(&pool, "mine@example.com").await;
    let theirs = an_org(&pool, "theirs@example.com").await;
    let projects = backend::projects(&pg);
    let ours = a_project(&projects, mine, "storefront").await;

    assert_eq!(
        projects
            .rename(theirs, ours, &a_slug("stolen"))
            .await
            .expect("the rename"),
        Renamed::Gone
    );
    assert_eq!(
        projects
            .assign(theirs, ours, None)
            .await
            .expect("the assign"),
        Assigned::Gone
    );
    assert!(!projects.forget(theirs, ours).await.expect("the forget"));
    assert_eq!(named(&projects, mine, "storefront").await, ours);
}

#[tokio::test]
async fn forgetting_a_project_takes_its_keys_and_leaves_the_team_standing() {
    let Some(pool) = control_plane("projects_forget").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let projects = backend::projects(&pg);
    let storefront = a_project(&projects, org, "storefront").await;
    let backend = a_team(&pool, org, "Backend").await;
    projects
        .assign(org, storefront, Some(backend))
        .await
        .expect("the hand over");
    PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000))
        .issue(org, Some(storefront.as_uuid()))
        .await
        .expect("a key");

    assert!(projects.forget(org, storefront).await.expect("the forget"));

    assert!(projects.of(org).await.expect("the projects").is_empty());
    assert_eq!(keys_live(&pool, storefront).await, 0);
    let teams: i64 = sqlx::query("SELECT count(*) AS value FROM teams")
        .fetch_one(&pool)
        .await
        .expect("counting the teams")
        .get("value");
    assert_eq!(teams, 1, "the team outlives the project it owned");
}

#[tokio::test]
async fn a_catalogue_reads_in_the_order_a_person_would_look_for_a_name_in() {
    let Some(pool) = control_plane("projects_order").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let projects = backend::projects(&pg);
    for name in ["storefront", "ads", "payments-eu"] {
        a_project(&projects, org, name).await;
    }

    let held = projects.of(org).await.expect("the projects");

    assert_eq!(
        held.iter().map(|p| &*p.name).collect::<Vec<_>>(),
        vec!["ads", "payments-eu", "storefront"]
    );
}

#[tokio::test]
async fn a_read_of_an_organisation_with_nothing_in_it_is_an_empty_catalogue() {
    let Some(pool) = control_plane("projects_none").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;

    let held: Result<_, StoreError> = backend::projects(&pg).of(org).await;

    assert!(held.expect("the projects").is_empty());
}
