use ferristrace_domain::{
    AccountId, Action, Actor, ApiKeys, AuditEntry, AuditTrail, Detail, ExportSource, Fingerprint,
    IssueDecisions, IssuedKey, KeyName, KeyScopes, KeyTtl, OrgRole, ProjectId, ProjectName, Roll,
    SecretHash, Snapshot, Squads, Status, Target, TeamName, TenantId,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;

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

fn backend_of(pool: &PgPool) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(NOW))
}

async fn an_account(pool: &PgPool, email: &str) -> AccountId {
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(account)
        .bind(email)
        .execute(pool)
        .await
        .expect("inserting an account");
    AccountId::new(account)
}

struct Furnished {
    org: TenantId,
    owner: AccountId,
    member: AccountId,
    project: ProjectId,
    issued: IssuedKey,
}

async fn furnished(pool: &PgPool, name: &str, tag: &str) -> Furnished {
    let pg = backend_of(pool);
    let owner_email = format!("owner-{tag}@example.com");
    let owner = an_account(pool, &owner_email).await;
    let org = backend::memberships(&pg)
        .found(name, &owner_email)
        .await
        .expect("the organisation is founded");
    let member = an_account(pool, &format!("member-{tag}@example.com")).await;
    backend::roster(&pg)
        .join(org, member, OrgRole::Member)
        .await
        .expect("the member joins");
    let project = backend::projects(&pg)
        .claim(
            org,
            &ProjectName::try_from(format!("web-{tag}").as_str()).expect("a slug"),
        )
        .await
        .expect("a project");
    backend::teams(&pg)
        .create(
            org,
            &TeamName::try_from(format!("team-{tag}").as_str()).expect("a team name"),
        )
        .await
        .expect("a team");
    let team = backend::teams(&pg).of(org).await.expect("the teams")[0].id;
    backend::teams(&pg)
        .seat(org, team, &[member])
        .await
        .expect("seated");
    backend::teams(&pg)
        .own(org, team, &[project])
        .await
        .expect("owned");
    let issued = backend::api_keys(&pg)
        .issue(
            org,
            Some(project),
            &KeyName::try_from(format!("key-{tag}").as_str()).expect("a key name"),
            &KeyScopes::ingest(),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("a key");
    backend::decisions(&pg)
        .decide(
            project,
            Fingerprint::of(tag, "TimeoutError", "deadline"),
            Status::Resolved,
            owner,
            NOW,
        )
        .await
        .expect("a decision");
    backend::audit(&pg)
        .record(&AuditEntry {
            org,
            actor: Actor::Account(owner),
            action: Action::OrgRenamed,
            target: Target::org(org),
            detail: Detail::default().with("to", name),
        })
        .await
        .expect("an entry");
    Furnished {
        org,
        owner,
        member,
        project,
        issued,
    }
}

async fn snapshot(pool: &PgPool, org: TenantId) -> Snapshot {
    backend::export(&backend_of(pool))
        .snapshot(org)
        .await
        .expect("the snapshot")
}

#[tokio::test]
async fn the_snapshot_of_an_org_holds_all_of_its_control_plane() {
    let Some(pool) = control_plane("export_whole").await else {
        return;
    };
    let ours = furnished(&pool, "Acme", "a").await;

    let held = snapshot(&pool, ours.org).await;

    assert_eq!(held.org.id, ours.org);
    assert_eq!(&*held.org.name, "Acme");
    assert_eq!(
        held.projects.iter().map(|p| p.id).collect::<Vec<_>>(),
        vec![ours.project]
    );
    assert_eq!(held.teams.len(), 1);
    assert_eq!(held.teams[0].members, vec![ours.member]);
    assert_eq!(held.teams[0].projects.len(), 1);
    let mut members: Vec<_> = held.members.iter().map(|m| m.account).collect();
    members.sort();
    let mut expected = vec![ours.owner, ours.member];
    expected.sort();
    assert_eq!(members, expected);
    assert_eq!(held.keys.len(), 1);
    assert_eq!(held.keys[0].id, ours.issued.key.id);
    assert_eq!(held.decisions.len(), 1);
    assert_eq!(held.decisions[0].project, ours.project);
    assert!(
        held.audit
            .iter()
            .any(|logged| logged.entry.action == Action::OrgRenamed)
    );
}

#[tokio::test]
async fn the_snapshot_of_org_a_holds_nothing_of_org_b() {
    let Some(pool) = control_plane("export_isolated").await else {
        return;
    };
    let ours = furnished(&pool, "Acme", "a").await;
    let theirs = furnished(&pool, "Globex", "b").await;

    let held = snapshot(&pool, ours.org).await;

    assert_eq!(held.org.id, ours.org);
    assert!(held.projects.iter().all(|p| p.org == ours.org));
    assert!(held.projects.iter().all(|p| p.id != theirs.project));
    assert!(held.teams.iter().all(|t| &*t.name == "team-a"));
    assert!(
        held.teams
            .iter()
            .all(|t| !t.members.contains(&theirs.member) && !t.members.contains(&theirs.owner))
    );
    assert!(
        held.members
            .iter()
            .all(|m| m.account != theirs.owner && m.account != theirs.member)
    );
    assert!(held.keys.iter().all(|k| k.id != theirs.issued.key.id));
    assert!(held.decisions.iter().all(|d| d.project != theirs.project));
    assert!(held.audit.iter().all(|logged| logged.entry.org == ours.org));
    assert!(held.audit.iter().all(|logged| {
        logged.entry.detail.get("to").flatten() != Some("Globex")
            && logged.entry.target != Target::org(theirs.org)
    }));
}

#[tokio::test]
async fn the_snapshot_of_a_key_holds_neither_its_secret_nor_its_digest() {
    let Some(pool) = control_plane("export_no_secret").await else {
        return;
    };
    let ours = furnished(&pool, "Acme", "a").await;
    let plaintext = ours.issued.secret.exposed().to_owned();
    let (_, offered) = ours.issued.secret.split().expect("a secret with a prefix");
    let digest = SecretHash::of(offered);

    let held = snapshot(&pool, ours.org).await;

    let shown = format!("{held:?}");
    assert!(!shown.contains(&plaintext));
    assert!(!shown.contains(offered));
    assert!(!shown.contains(&hex::encode(digest.as_bytes())));
    assert!(!shown.contains(&format!("{:?}", digest.as_bytes())));
    assert_eq!(held.keys[0].prefix, ours.issued.key.prefix);
}

#[tokio::test]
async fn an_org_that_does_not_exist_has_no_snapshot() {
    let Some(pool) = control_plane("export_absent").await else {
        return;
    };

    let held = backend::export(&backend_of(&pool))
        .snapshot(TenantId::new(Uuid::new_v4()))
        .await;

    assert!(held.is_err());
}

#[tokio::test]
async fn a_snapshot_reads_more_audit_entries_than_one_page_holds() {
    let Some(pool) = control_plane("export_long_trail").await else {
        return;
    };
    let ours = furnished(&pool, "Acme", "a").await;
    let pg = backend_of(&pool);
    for index in 0..450 {
        backend::audit(&pg)
            .record(&AuditEntry {
                org: ours.org,
                actor: Actor::Account(ours.owner),
                action: Action::OrgRenamed,
                target: Target::org(ours.org),
                detail: Detail::default().with("to", format!("name-{index}")),
            })
            .await
            .expect("an entry");
    }

    let held = snapshot(&pool, ours.org).await;

    assert!(held.audit.len() >= 450);
    let ids: std::collections::HashSet<_> = held.audit.iter().map(|logged| logged.id).collect();
    assert_eq!(ids.len(), held.audit.len());
}
