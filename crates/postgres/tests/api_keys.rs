use ferristrace_domain::{
    AccountId, ApiKeys, ApiSecret, ExpiringKey, ExpiryThreshold, IngestKey, KeyName, KeyScope,
    KeyScopes, KeyTtl, ProjectId, ProjectName, Projects, ReadGrant, TenantDirectory, TenantId,
    UnknownKey,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres, PostgresDirectory};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_000;
const NANOS_PER_SECOND: u64 = 1_000_000_000;
const NANOS_PER_DAY: u64 = 86_400 * NANOS_PER_SECOND;

fn now_nanos() -> u64 {
    NOW * NANOS_PER_SECOND
}

async fn set_expiry(pool: &PgPool, id: Uuid, expires_at_unix_nanos: Option<i64>) {
    let done = sqlx::query("UPDATE api_keys SET expires_at = $2 WHERE id = $1")
        .bind(id)
        .bind(expires_at_unix_nanos)
        .execute(pool)
        .await
        .expect("setting the expiry directly");
    assert_eq!(
        done.rows_affected(),
        1,
        "the key row was not found to update"
    );
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

fn backend_of(pool: &PgPool) -> Postgres<FixedClock> {
    Postgres::new(pool.clone(), FixedClock::at(NOW))
}

async fn an_org(pool: &PgPool, owner: &str) -> (TenantId, AccountId) {
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
        .bind(account)
        .bind(owner)
        .execute(pool)
        .await
        .expect("inserting an account");
    let org = backend::memberships(&backend_of(pool))
        .found("Acme", owner)
        .await
        .expect("the organisation is founded");
    (org, AccountId::new(account))
}

async fn a_project(pool: &PgPool, org: TenantId, name: &str) -> ProjectId {
    let pg = backend_of(pool);
    let projects = backend::projects(&pg);
    projects
        .create(org, &ProjectName::try_from(name).expect("a slug"))
        .await
        .expect("the create");
    projects
        .of(org)
        .await
        .expect("the projects")
        .into_iter()
        .find(|project| &*project.name == name)
        .expect("the project is there")
        .id
}

fn a_name(spelling: &str) -> KeyName {
    KeyName::try_from(spelling).expect("a name")
}

fn scopes(of: &[KeyScope]) -> KeyScopes {
    KeyScopes::new(of).expect("scopes")
}

fn as_ingest(secret: &ApiSecret) -> IngestKey {
    IngestKey::from_plaintext(secret.exposed())
}

#[tokio::test]
async fn an_issued_key_is_listed_with_its_name_scopes_and_creator_but_no_secret() {
    let Some(pool) = control_plane("api_keys_issue").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let storefront = a_project(&pool, org, "storefront").await;
    let keys = backend::api_keys(&pg);

    let issued = keys
        .issue(
            org,
            Some(storefront),
            &a_name("edge collector"),
            &scopes(&[KeyScope::Read, KeyScope::Ingest]),
            owner,
            KeyTtl::Days30,
        )
        .await
        .expect("the issue");
    let listed = keys.list(org).await.expect("the list");

    let (prefix, secret) = issued.secret.split().expect("a well formed secret");
    assert_eq!(prefix, &*issued.key.prefix);
    assert!(prefix.starts_with("ft_live_") && prefix.len() == "ft_live_".len() + 12);
    assert_eq!(secret.len(), 64);
    assert_eq!(listed, vec![issued.key.clone()]);
    assert_eq!(&*listed[0].name, "edge collector");
    assert_eq!(listed[0].project, Some(storefront));
    assert_eq!(
        listed[0].scopes.as_slice(),
        &[KeyScope::Ingest, KeyScope::Read]
    );
    assert_eq!(listed[0].created_by, Some(owner));
    assert_eq!(listed[0].last_used_at_unix_nanos, None);
    assert!(listed[0].created_at_unix_nanos > 1_600_000_000_000_000_000);
    assert_eq!(
        listed[0].expires_at_unix_nanos,
        KeyTtl::Days30.expires_at(listed[0].created_at_unix_nanos)
    );
    let stored: Vec<u8> = sqlx::query("SELECT secret_digest FROM api_keys WHERE prefix = $1")
        .bind(prefix)
        .fetch_one(&pool)
        .await
        .expect("the row")
        .get("secret_digest");
    assert_ne!(stored, secret.as_bytes(), "only a digest is kept");
}

#[tokio::test]
async fn a_key_issued_without_a_ttl_carries_no_expiry() {
    let Some(pool) = control_plane("api_keys_never_expires").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let keys = backend::api_keys(&pg);

    let issued = keys
        .issue(
            org,
            None,
            &a_name("collector"),
            &KeyScopes::ingest(),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");

    assert_eq!(issued.key.expires_at_unix_nanos, None);
}

#[tokio::test]
async fn a_read_key_grants_its_organisation_and_project_and_notes_its_use() {
    let Some(pool) = control_plane("api_keys_grant").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let storefront = a_project(&pool, org, "storefront").await;
    let keys = backend::api_keys(&pg);
    let issued = keys
        .issue(
            org,
            Some(storefront),
            &a_name("dashboards"),
            &scopes(&[KeyScope::Read]),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");

    let grant = keys.grant_for(&issued.secret).await.expect("a lookup");

    assert_eq!(
        grant,
        Some(ReadGrant {
            org,
            project: Some(storefront),
        })
    );
    let listed = keys.list(org).await.expect("the list");
    assert_eq!(listed[0].last_used_at_unix_nanos, Some(NOW * 1_000_000_000));
}

#[tokio::test]
async fn an_ingest_key_a_wrong_secret_and_a_malformed_one_grant_nothing() {
    let Some(pool) = control_plane("api_keys_refuse").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let keys = backend::api_keys(&pg);
    let ingest = keys
        .issue(
            org,
            None,
            &a_name("collector"),
            &KeyScopes::ingest(),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");
    let read = keys
        .issue(
            org,
            None,
            &a_name("dashboards"),
            &scopes(&[KeyScope::Read]),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");
    let (prefix, _) = read.secret.split().expect("a secret");
    let wrong = ApiSecret::from_plaintext(&format!("{prefix}.{}", "0".repeat(64)));

    assert_eq!(keys.grant_for(&ingest.secret).await, Ok(None));
    assert_eq!(keys.grant_for(&wrong).await, Ok(None));
    assert_eq!(
        keys.grant_for(&ApiSecret::from_plaintext("not-a-key"))
            .await,
        Ok(None)
    );
}

#[tokio::test]
async fn a_read_only_key_does_not_resolve_through_the_ingest_directory() {
    let Some(pool) = control_plane("api_keys_directory").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let keys = backend::api_keys(&pg);
    let read = keys
        .issue(
            org,
            None,
            &a_name("dashboards"),
            &scopes(&[KeyScope::Read]),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");
    let both = keys
        .issue(
            org,
            None,
            &a_name("everything"),
            &scopes(&[KeyScope::Ingest, KeyScope::Read]),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(NOW));

    let refused = directory.resolve(&as_ingest(&read.secret)).await;
    let resolved = directory.resolve(&as_ingest(&both.secret)).await;

    assert_eq!(refused, Err(UnknownKey));
    assert_eq!(resolved.map(|scope| scope.tenant), Ok(org));
}

#[tokio::test]
async fn a_key_issued_from_the_command_line_ingests_and_reads_nothing() {
    let Some(pool) = control_plane("api_keys_cli").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, _) = an_org(&pool, "owner@example.com").await;
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(NOW));

    let issued = directory.issue(org, None).await.expect("the issue");
    let listed = backend::api_keys(&pg).list(org).await.expect("the list");

    assert_eq!(
        directory.resolve(&issued).await.map(|scope| scope.tenant),
        Ok(org)
    );
    assert_eq!(
        backend::api_keys(&pg)
            .grant_for(&ApiSecret::from_plaintext(issued.exposed()))
            .await,
        Ok(None)
    );
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].scopes, KeyScopes::ingest());
    assert_eq!(listed[0].created_by, None);
}

#[tokio::test]
async fn a_revoked_key_neither_grants_nor_ingests_nor_is_listed() {
    let Some(pool) = control_plane("api_keys_revoke").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let keys = backend::api_keys(&pg);
    let issued = keys
        .issue(
            org,
            None,
            &a_name("everything"),
            &scopes(&[KeyScope::Ingest, KeyScope::Read]),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(NOW));
    assert!(directory.resolve(&as_ingest(&issued.secret)).await.is_ok());

    let revoked = keys.revoke(org, issued.key.id).await.expect("the revoke");
    let again = keys
        .revoke(org, issued.key.id)
        .await
        .expect("a second revoke");

    assert!(revoked);
    assert!(!again, "a revoked key is not revoked twice");
    assert_eq!(keys.grant_for(&issued.secret).await, Ok(None));
    assert_eq!(
        directory.resolve(&as_ingest(&issued.secret)).await,
        Err(UnknownKey)
    );
    assert_eq!(keys.list(org).await, Ok(Vec::new()));
}

#[tokio::test]
async fn one_organisation_neither_lists_nor_revokes_the_keys_of_another() {
    let Some(pool) = control_plane("api_keys_tenancy").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (mine, owner) = an_org(&pool, "mine@example.com").await;
    let (theirs, _) = an_org(&pool, "theirs@example.com").await;
    let keys = backend::api_keys(&pg);
    let issued = keys
        .issue(
            mine,
            None,
            &a_name("collector"),
            &KeyScopes::ingest(),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");

    assert_eq!(keys.list(theirs).await, Ok(Vec::new()));
    assert_eq!(keys.revoke(theirs, issued.key.id).await, Ok(false));
    assert_eq!(keys.list(mine).await.map(|listed| listed.len()), Ok(1));
}

#[tokio::test]
async fn a_key_entering_its_seven_day_window_is_returned_once_then_not_again() {
    let Some(pool) = control_plane("api_keys_notice_7d").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let keys = backend::api_keys(&pg);
    let issued = keys
        .issue(
            org,
            None,
            &a_name("ci"),
            &KeyScopes::ingest(),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");
    let expires_at = now_nanos() + 5 * NANOS_PER_DAY;
    set_expiry(
        &pool,
        issued.key.id.as_uuid(),
        Some(i64::try_from(expires_at).expect("fits")),
    )
    .await;

    let due = keys
        .due_for_expiry_notice(now_nanos())
        .await
        .expect("the read");
    assert_eq!(
        due,
        vec![(
            ExpiringKey {
                id: issued.key.id,
                org,
                name: "ci".into(),
                created_by: Some(owner),
            },
            ExpiryThreshold::Days7
        )]
    );

    keys.mark_expiry_notice_sent(issued.key.id, ExpiryThreshold::Days7)
        .await
        .expect("marking it sent");
    let again = keys
        .due_for_expiry_notice(now_nanos())
        .await
        .expect("the read");
    assert!(again.is_empty(), "the seven day notice was already sent");
}

#[tokio::test]
async fn a_key_closer_than_a_day_out_is_due_for_both_notices_at_once() {
    let Some(pool) = control_plane("api_keys_notice_both").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let keys = backend::api_keys(&pg);
    let issued = keys
        .issue(
            org,
            None,
            &a_name("ci"),
            &KeyScopes::ingest(),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");
    let expires_at = now_nanos() + NANOS_PER_DAY / 2;
    set_expiry(
        &pool,
        issued.key.id.as_uuid(),
        Some(i64::try_from(expires_at).expect("fits")),
    )
    .await;

    let mut due = keys
        .due_for_expiry_notice(now_nanos())
        .await
        .expect("the read");
    due.sort_by_key(|(_, threshold)| *threshold);

    assert_eq!(
        due.into_iter()
            .map(|(_, threshold)| threshold)
            .collect::<Vec<_>>(),
        vec![ExpiryThreshold::Days7, ExpiryThreshold::Day1]
    );
}

#[tokio::test]
async fn a_revoked_key_is_never_due_for_a_notice() {
    let Some(pool) = control_plane("api_keys_notice_revoked").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let keys = backend::api_keys(&pg);
    let issued = keys
        .issue(
            org,
            None,
            &a_name("ci"),
            &KeyScopes::ingest(),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");
    let expires_at = now_nanos() + NANOS_PER_DAY / 2;
    set_expiry(
        &pool,
        issued.key.id.as_uuid(),
        Some(i64::try_from(expires_at).expect("fits")),
    )
    .await;
    keys.revoke(org, issued.key.id).await.expect("the revoke");

    assert!(
        keys.due_for_expiry_notice(now_nanos())
            .await
            .expect("the read")
            .is_empty()
    );
}

#[tokio::test]
async fn a_key_past_its_deadline_is_due_for_auto_revoke_until_it_is_revoked() {
    let Some(pool) = control_plane("api_keys_auto_revoke").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, owner) = an_org(&pool, "owner@example.com").await;
    let keys = backend::api_keys(&pg);
    let issued = keys
        .issue(
            org,
            None,
            &a_name("ci"),
            &KeyScopes::ingest(),
            owner,
            KeyTtl::Never,
        )
        .await
        .expect("the issue");
    let expires_at = now_nanos() - NANOS_PER_SECOND;
    set_expiry(
        &pool,
        issued.key.id.as_uuid(),
        Some(i64::try_from(expires_at).expect("fits")),
    )
    .await;

    let due = keys
        .due_for_auto_revoke(now_nanos())
        .await
        .expect("the read");
    assert_eq!(
        due,
        vec![ExpiringKey {
            id: issued.key.id,
            org,
            name: "ci".into(),
            created_by: Some(owner),
        }]
    );

    keys.revoke(org, issued.key.id).await.expect("the revoke");
    let again = keys
        .due_for_auto_revoke(now_nanos())
        .await
        .expect("the read");
    assert!(again.is_empty(), "a revoked key is no longer due");
}
