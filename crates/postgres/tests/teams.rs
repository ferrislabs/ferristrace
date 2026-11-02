use ferristrace_domain::{
    AccountId, Named, OrgRole, ProjectId, ProjectName, Roll, Squads, Team, TeamId, TeamName,
    TenantId,
};
use ferristrace_postgres::{
    CONTROL_PLANE, PostgresMemberships, PostgresProjects, PostgresRoster, PostgresTeams,
};
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
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'chosen')")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await
        .expect("inserting an account");
    AccountId::new(id)
}

async fn an_org(pool: &PgPool, owner: &str) -> TenantId {
    an_account(pool, owner).await;
    PostgresMemberships::new(pool.clone())
        .found("Acme", owner)
        .await
        .expect("the organisation is founded")
}

async fn a_member(pool: &PgPool, org: TenantId, email: &str) -> AccountId {
    let account = an_account(pool, email).await;
    PostgresMemberships::new(pool.clone())
        .grant(org, email, OrgRole::Member)
        .await
        .expect("the grant");
    account
}

async fn a_project(pool: &PgPool, org: TenantId, name: &str) -> ProjectId {
    PostgresProjects::new(pool.clone())
        .claim(org, &ProjectName::try_from(name).expect("an admitted name"))
        .await
        .expect("creating the project")
}

fn a_name(spelling: &str) -> TeamName {
    TeamName::try_from(spelling).expect("an admitted name")
}

async fn a_team(teams: &PostgresTeams, org: TenantId, name: &str) -> TeamId {
    assert_eq!(
        teams.create(org, &a_name(name)).await.expect("the create"),
        Named::Free
    );
    named(teams, org, name).await.id
}

async fn named(teams: &PostgresTeams, org: TenantId, name: &str) -> Team {
    teams
        .of(org)
        .await
        .expect("the teams are read")
        .into_iter()
        .find(|team| &*team.name == name)
        .unwrap_or_else(|| panic!("{name} is on the roll"))
}

async fn team_of(pool: &PgPool, project: ProjectId) -> Option<Uuid> {
    sqlx::query("SELECT team_id AS team_id FROM projects WHERE id = $1")
        .bind(project.as_uuid())
        .fetch_one(pool)
        .await
        .expect("reading the project")
        .get("team_id")
}

#[tokio::test]
async fn a_created_team_reads_back_with_nobody_on_it_and_nothing_owned() {
    let Some(pool) = control_plane("teams_create").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let teams = PostgresTeams::new(pool.clone());

    teams
        .create(org, &a_name("Backend"))
        .await
        .expect("the create");

    let team = named(&teams, org, "Backend").await;
    assert!(team.members.is_empty());
    assert!(team.projects.is_empty());
    assert!(
        team.created_at_unix_nanos > 1_600_000_000_000_000_000,
        "the column holds nanoseconds, not seconds: {}",
        team.created_at_unix_nanos
    );
}

#[tokio::test]
async fn a_name_one_organisation_holds_is_still_free_in_another() {
    let Some(pool) = control_plane("teams_names").await else {
        return;
    };
    let mine = an_org(&pool, "mine@example.com").await;
    let theirs = an_org(&pool, "theirs@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    a_team(&teams, mine, "Backend").await;

    let again = teams
        .create(mine, &a_name("Backend"))
        .await
        .expect("the second create");
    let elsewhere = teams
        .create(theirs, &a_name("Backend"))
        .await
        .expect("the create in another organisation");

    assert_eq!(again, Named::Taken);
    assert_eq!(elsewhere, Named::Free);
    assert_eq!(teams.of(mine).await.expect("the teams").len(), 1);
}

#[tokio::test]
async fn a_team_renamed_to_the_name_it_already_has_is_not_a_clash() {
    let Some(pool) = control_plane("teams_rename_self").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, org, "Backend").await;

    let again = teams
        .rename(org, backend, &a_name("Backend"))
        .await
        .expect("the rename");

    assert_eq!(again, Named::Free);
    assert_eq!(&*named(&teams, org, "Backend").await.name, "Backend");
}

#[tokio::test]
async fn renaming_a_team_onto_another_teams_name_is_refused() {
    let Some(pool) = control_plane("teams_rename_clash").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, org, "Backend").await;
    a_team(&teams, org, "Payments").await;

    let refused = teams
        .rename(org, backend, &a_name("Payments"))
        .await
        .expect("the rename");

    assert_eq!(refused, Named::Taken);
    assert_eq!(&*named(&teams, org, "Backend").await.name, "Backend");
}

#[tokio::test]
async fn seating_writes_the_whole_list_and_a_second_one_replaces_it() {
    let Some(pool) = control_plane("teams_seat").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let first = a_member(&pool, org, "first@example.com").await;
    let second = a_member(&pool, org, "second@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, org, "Backend").await;

    assert!(
        teams
            .seat(org, backend, &[first, second])
            .await
            .expect("the first seating")
    );
    let both = named(&teams, org, "Backend").await.members;
    assert!(
        teams
            .seat(org, backend, &[second])
            .await
            .expect("the second seating")
    );

    assert_eq!(both.len(), 2);
    assert_eq!(
        named(&teams, org, "Backend").await.members,
        vec![second],
        "whoever is left off the list asked for is off the team"
    );
}

#[tokio::test]
async fn leaving_the_organisation_takes_you_off_its_teams() {
    let Some(pool) = control_plane("teams_leaving").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let leaver = a_member(&pool, org, "leaver@example.com").await;
    let stayer = a_member(&pool, org, "stayer@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, org, "Backend").await;
    teams
        .seat(org, backend, &[leaver, stayer])
        .await
        .expect("the seating");

    sqlx::query("DELETE FROM org_members WHERE org_id = $1 AND account_id = $2")
        .bind(org.as_uuid())
        .bind(leaver.as_uuid())
        .execute(&pool)
        .await
        .expect("the membership goes");

    assert_eq!(named(&teams, org, "Backend").await.members, vec![stayer]);
}

#[tokio::test]
async fn an_account_of_another_organisation_is_dropped_rather_than_seated() {
    let Some(pool) = control_plane("teams_stranger").await else {
        return;
    };
    let mine = an_org(&pool, "mine@example.com").await;
    let theirs = an_org(&pool, "theirs@example.com").await;
    let ours = a_member(&pool, mine, "ours@example.com").await;
    let stranger = a_member(&pool, theirs, "stranger@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, mine, "Backend").await;

    teams
        .seat(mine, backend, &[ours, stranger])
        .await
        .expect("the seating");

    assert_eq!(
        named(&teams, mine, "Backend").await.members,
        vec![ours],
        "a team must not hold somebody the organisation does not"
    );
}

#[tokio::test]
async fn a_project_has_one_owner_team_so_claiming_it_takes_it_off_the_other() {
    let Some(pool) = control_plane("teams_own").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let storefront = a_project(&pool, org, "storefront").await;
    let payments = a_project(&pool, org, "payments-eu").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, org, "Backend").await;
    let mobile = a_team(&teams, org, "Mobile").await;

    teams
        .own(org, backend, &[storefront, payments])
        .await
        .expect("the first claim");
    teams
        .own(org, mobile, &[storefront])
        .await
        .expect("the second claim");

    assert_eq!(
        team_of(&pool, storefront).await,
        Some(mobile.as_uuid()),
        "the last team to claim it owns it"
    );
    assert_eq!(team_of(&pool, payments).await, Some(backend.as_uuid()));
    let held = named(&teams, org, "Backend").await.projects;
    assert_eq!(held.len(), 1);
    assert_eq!(&*held[0].name, "payments-eu");
}

#[tokio::test]
async fn a_project_left_off_the_list_asked_for_is_let_go() {
    let Some(pool) = control_plane("teams_disown").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let storefront = a_project(&pool, org, "storefront").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, org, "Backend").await;
    teams
        .own(org, backend, &[storefront])
        .await
        .expect("the claim");

    teams.own(org, backend, &[]).await.expect("the empty claim");

    assert_eq!(team_of(&pool, storefront).await, None);
    assert!(named(&teams, org, "Backend").await.projects.is_empty());
}

#[tokio::test]
async fn disbanding_a_team_leaves_its_projects_behind_with_no_owner() {
    let Some(pool) = control_plane("teams_disband").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let storefront = a_project(&pool, org, "storefront").await;
    let member = a_member(&pool, org, "member@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, org, "Backend").await;
    teams
        .own(org, backend, &[storefront])
        .await
        .expect("the claim");
    teams
        .seat(org, backend, &[member])
        .await
        .expect("the seating");

    assert!(teams.disband(org, backend).await.expect("the disband"));

    assert!(teams.of(org).await.expect("the teams").is_empty());
    assert_eq!(team_of(&pool, storefront).await, None);
    let left: i64 = sqlx::query("SELECT count(*) AS value FROM team_members")
        .fetch_one(&pool)
        .await
        .expect("counting the seats")
        .get("value");
    assert_eq!(left, 0, "the seats went with the team");
}

#[tokio::test]
async fn a_team_of_another_organisation_is_not_there_to_arrange() {
    let Some(pool) = control_plane("teams_theirs").await else {
        return;
    };
    let mine = an_org(&pool, "mine@example.com").await;
    let theirs = an_org(&pool, "theirs@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    let ours = a_team(&teams, mine, "Backend").await;

    assert!(!teams.seat(theirs, ours, &[]).await.expect("the seating"));
    assert!(!teams.own(theirs, ours, &[]).await.expect("the claim"));
    assert!(!teams.disband(theirs, ours).await.expect("the disband"));
    assert_eq!(
        teams.of(mine).await.expect("the teams").len(),
        1,
        "and it is still there"
    );
}

#[tokio::test]
async fn deleting_an_organisation_takes_its_teams_with_it() {
    let Some(pool) = control_plane("teams_cascade").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let member = a_member(&pool, org, "member@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, org, "Backend").await;
    teams
        .seat(org, backend, &[member])
        .await
        .expect("the seating");

    sqlx::query("DELETE FROM orgs WHERE id = $1")
        .bind(org.as_uuid())
        .execute(&pool)
        .await
        .expect("deleting the organisation");

    for table in ["teams", "team_members"] {
        let left: i64 = sqlx::query(AssertSqlSafe(format!(
            "SELECT count(*) AS value FROM {table}"
        )))
        .fetch_one(&pool)
        .await
        .expect("counting")
        .get("value");
        assert_eq!(left, 0, "{table} kept rows for a deleted organisation");
    }
}

#[tokio::test]
async fn a_member_removed_through_the_roster_is_off_the_teams_too() {
    let Some(pool) = control_plane("teams_roster").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    let leaver = a_member(&pool, org, "leaver@example.com").await;
    let teams = PostgresTeams::new(pool.clone());
    let backend = a_team(&teams, org, "Backend").await;
    teams
        .seat(org, backend, &[leaver])
        .await
        .expect("the seating");

    let removed = PostgresRoster::new(
        pool.clone(),
        ferristrace_infra::Argon2Hasher::with_cost(8, 1, 1).expect("a cheap hasher"),
        ferristrace_infra::SystemClock,
    )
    .remove(org, leaver)
    .await
    .expect("the removal");

    assert!(removed);
    assert!(named(&teams, org, "Backend").await.members.is_empty());
}
