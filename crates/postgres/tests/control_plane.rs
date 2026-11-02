use ferristrace_application::registry::{RepoFor, Settle, UnitOfWork, domain};
use ferristrace_domain::Allowance;
use ferristrace_domain::{
    ControlPlane, DecompressedBytes, Email, IngestKey, Issuer, OrgName, ProjectName, Projects,
    ProvisionPolicy, ResolvedIdentity, Scope, SecretHash, Subject, TenantDirectory, TenantId,
    UnknownKey, testing,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{
    AccountError, CONTROL_PLANE, OVERAGE_FEATURE, Postgres, PostgresDirectory, ProjectError,
    SELF_HOSTED_PLAN,
};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

#[macro_use]
mod backend;

fn a_slug(name: &str) -> ProjectName {
    ProjectName::try_from(name).expect("an admitted name")
}

async fn migrated(schema: &str) -> Option<PgPool> {
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

#[tokio::test]
async fn the_self_hosted_plan_leaves_every_limit_open() {
    let Some(pool) = migrated("control_plan").await else {
        return;
    };

    let row = sqlx::query(
        "SELECT ingest_gb, retention_days, max_members, features \
         FROM plans WHERE slug = $1",
    )
    .bind(SELF_HOSTED_PLAN)
    .fetch_one(&pool)
    .await
    .expect("the default plan exists");

    assert_eq!(row.get::<Option<i32>, _>("ingest_gb"), None);
    assert_eq!(row.get::<Option<i32>, _>("retention_days"), None);
    assert_eq!(row.get::<Option<i32>, _>("max_members"), None);
    assert_eq!(row.get::<Vec<String>, _>("features"), Vec::<String>::new());
}

#[tokio::test]
async fn an_account_may_hold_only_a_password() {
    let Some(pool) = migrated("control_password_only").await else {
        return;
    };

    sqlx::query(
        "INSERT INTO accounts (id, email, password_hash) \
         VALUES (gen_random_uuid(), 'local@example.com', '$argon2id$hash')",
    )
    .execute(&pool)
    .await
    .expect("a password is a way in on its own");
}

#[tokio::test]
async fn an_account_may_now_exist_before_any_way_in_is_linked() {
    let Some(pool) = migrated("control_no_way_in").await else {
        return;
    };

    sqlx::query(
        "INSERT INTO accounts (id, email, password_hash) \
         VALUES (gen_random_uuid(), 'nobody@example.com', NULL)",
    )
    .execute(&pool)
    .await
    .expect("the cross table constraint is gone, so this row is accepted");

    let linked: i64 = sqlx::query("SELECT count(*) AS n FROM identity_links")
        .fetch_one(&pool)
        .await
        .expect("counting links")
        .get("n");
    assert_eq!(linked, 0, "and it can authenticate through nothing");
}

#[tokio::test]
async fn an_api_key_prefix_is_claimed_once() {
    let Some(pool) = migrated("control_prefix").await else {
        return;
    };
    let org = seeded_org(&pool).await;

    sqlx::query(
        "INSERT INTO api_keys (id, org_id, prefix, hash) \
         VALUES (gen_random_uuid(), $1, 'ft_live_abc123', '$argon2id$first')",
    )
    .bind(org)
    .execute(&pool)
    .await
    .expect("the first key takes the prefix");

    let refused = sqlx::query(
        "INSERT INTO api_keys (id, org_id, prefix, hash) \
         VALUES (gen_random_uuid(), $1, 'ft_live_abc123', '$argon2id$second')",
    )
    .bind(org)
    .execute(&pool)
    .await;

    assert!(
        refused.is_err(),
        "two keys shared one prefix, so a lookup by prefix is no longer a single row"
    );
}

#[tokio::test]
async fn an_entitlement_refuses_zero_but_accepts_nothing_at_all() {
    let Some(pool) = migrated("control_entitlement").await else {
        return;
    };

    let refused = sqlx::query(
        "INSERT INTO plans (id, slug, name, ingest_gb) \
         VALUES (gen_random_uuid(), 'broken', 'Broken', 0)",
    )
    .execute(&pool)
    .await;
    let accepted = sqlx::query(
        "INSERT INTO plans (id, slug, name, ingest_gb) \
         VALUES (gen_random_uuid(), 'unlimited', 'Unlimited', NULL)",
    )
    .execute(&pool)
    .await;

    assert!(refused.is_err(), "a zero allowance is not a limit");
    assert!(accepted.is_ok(), "NULL must stand for unlimited");
}

#[tokio::test]
async fn dropping_an_org_takes_its_projects_and_keys() {
    let Some(pool) = migrated("control_cascade").await else {
        return;
    };
    let org = seeded_org(&pool).await;
    sqlx::query("INSERT INTO projects (id, org_id, name) VALUES (gen_random_uuid(), $1, 'web')")
        .bind(org)
        .execute(&pool)
        .await
        .expect("a project");
    sqlx::query(
        "INSERT INTO api_keys (id, org_id, prefix, hash) \
         VALUES (gen_random_uuid(), $1, 'ft_live_xyz', '$argon2id$hash')",
    )
    .bind(org)
    .execute(&pool)
    .await
    .expect("a key");

    sqlx::query("DELETE FROM orgs WHERE id = $1")
        .bind(org)
        .execute(&pool)
        .await
        .expect("the org goes");

    for table in ["projects", "api_keys"] {
        let left: i64 = sqlx::query(AssertSqlSafe(format!("SELECT count(*) AS n FROM {table}")))
            .fetch_one(&pool)
            .await
            .expect("counting")
            .get("n");
        assert_eq!(left, 0, "{table} kept rows for a deleted org");
    }
}

#[tokio::test]
async fn the_migrations_reverse_and_reapply() {
    let Some(pool) = migrated("control_cycle").await else {
        return;
    };

    CONTROL_PLANE
        .undo(&pool, 0)
        .await
        .expect("the migrations revert");
    let gone: i64 = sqlx::query(
        "SELECT count(*) AS n FROM information_schema.tables \
         WHERE table_schema = 'control_cycle' AND table_name = 'orgs'",
    )
    .fetch_one(&pool)
    .await
    .expect("counting the tables")
    .get("n");
    CONTROL_PLANE
        .run(&pool)
        .await
        .expect("the migrations apply again");

    assert_eq!(gone, 0, "the down step left the orgs table behind");
    assert!(
        sqlx::query("SELECT slug FROM plans WHERE slug = $1")
            .bind(SELF_HOSTED_PLAN)
            .fetch_optional(&pool)
            .await
            .expect("looking for the default plan")
            .is_some(),
        "reapplying the migrations did not restore the default plan"
    );
}

async fn seeded_org(pool: &PgPool) -> uuid::Uuid {
    let plan: uuid::Uuid = sqlx::query("SELECT id AS id FROM plans WHERE slug = $1")
        .bind(SELF_HOSTED_PLAN)
        .fetch_one(pool)
        .await
        .expect("the default plan")
        .get("id");
    let org = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO orgs (id, name, plan_id) VALUES ($1, 'Acme', $2)")
        .bind(org)
        .bind(plan)
        .execute(pool)
        .await
        .expect("an org");
    org
}

async fn plan_with(
    pool: &PgPool,
    slug: &str,
    ingest_gb: Option<i32>,
    features: &[&str],
) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO plans (id, slug, name, ingest_gb, features) VALUES ($1, $2, $2, $3, $4)",
    )
    .bind(id)
    .bind(slug)
    .bind(ingest_gb)
    .bind(features.iter().map(|f| (*f).to_owned()).collect::<Vec<_>>())
    .execute(pool)
    .await
    .expect("a plan");
    id
}

async fn org_on(pool: &PgPool, plan: uuid::Uuid) -> ferristrace_domain::TenantId {
    let org = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO orgs (id, name, plan_id) VALUES ($1, 'Acme', $2)")
        .bind(org)
        .bind(plan)
        .execute(pool)
        .await
        .expect("an org");
    ferristrace_domain::TenantId::new(org)
}

#[tokio::test]
async fn a_plan_without_an_ingest_limit_loads_as_unlimited() {
    let Some(pool) = migrated("control_unlimited").await else {
        return;
    };
    let plan = plan_with(&pool, "generous", None, &[]).await;
    let tenant = org_on(&pool, plan).await;

    let allowances = ferristrace_postgres::load_allowances(&pool)
        .await
        .expect("the allowances load");

    assert_eq!(allowances.get(&tenant), Some(&Allowance::Unlimited));
}

#[tokio::test]
async fn a_plan_without_the_overage_feature_caps_hard() {
    let Some(pool) = migrated("control_capped").await else {
        return;
    };
    let plan = plan_with(&pool, "free", Some(2), &[]).await;
    let tenant = org_on(&pool, plan).await;

    let allowances = ferristrace_postgres::load_allowances(&pool)
        .await
        .expect("the allowances load");

    assert_eq!(
        allowances.get(&tenant),
        Some(&Allowance::Capped(DecompressedBytes::new(
            2 * 1024 * 1024 * 1024
        )))
    );
}

#[tokio::test]
async fn a_plan_carrying_the_overage_feature_allows_the_overage() {
    let Some(pool) = migrated("control_overage").await else {
        return;
    };
    let plan = plan_with(&pool, "pro", Some(1), &[OVERAGE_FEATURE, "sso"]).await;
    let tenant = org_on(&pool, plan).await;

    let allowances = ferristrace_postgres::load_allowances(&pool)
        .await
        .expect("the allowances load");

    assert_eq!(
        allowances.get(&tenant),
        Some(&Allowance::Overage(DecompressedBytes::new(
            1024 * 1024 * 1024
        )))
    );
}

#[tokio::test]
async fn an_org_on_the_default_plan_is_unlimited() {
    let Some(pool) = migrated("control_default_plan").await else {
        return;
    };
    let tenant = ferristrace_domain::TenantId::new(seeded_org(&pool).await);

    let allowances = ferristrace_postgres::load_allowances(&pool)
        .await
        .expect("the allowances load");

    assert_eq!(allowances.get(&tenant), Some(&Allowance::Unlimited));
}
fn an_identity(issuer: Issuer, subject: &str, email: &str) -> ResolvedIdentity {
    ResolvedIdentity {
        issuer,
        subject: Subject::try_from(subject).expect("a subject"),
        email: Email::try_from(email).expect("an address"),
        email_verified: true,
    }
}

fn a_federated_identity(issuer: &str, subject: &str, email: &str) -> ResolvedIdentity {
    an_identity(Issuer::federated(issuer).expect("a url"), subject, email)
}

async fn account_count(pool: &PgPool) -> i64 {
    sqlx::query("SELECT count(*) AS n FROM accounts")
        .fetch_one(pool)
        .await
        .expect("counting accounts")
        .get("n")
}

async fn link_count(pool: &PgPool) -> i64 {
    sqlx::query("SELECT count(*) AS n FROM identity_links")
        .fetch_one(pool)
        .await
        .expect("counting links")
        .get("n")
}

#[tokio::test]
async fn provisioning_creates_an_account_and_its_link_together() {
    let Some(pool) = migrated("identity_provision").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let accounts = backend::accounts(&pg);
    let identity = a_federated_identity("https://id.example", "sub-1", "person@example.com");

    let account = accounts
        .provision(&identity, ProvisionPolicy::default())
        .await
        .expect("the identity is provisioned");

    assert_eq!(account_count(&pool).await, 1);
    assert_eq!(
        accounts.linked(&identity).await.expect("a lookup"),
        Some(account)
    );
}

#[tokio::test]
async fn provisioning_the_same_identity_twice_returns_the_same_account() {
    let Some(pool) = migrated("identity_twice").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let accounts = backend::accounts(&pg);
    let identity = a_federated_identity("https://id.example", "sub-1", "person@example.com");

    let first = accounts
        .provision(&identity, ProvisionPolicy::default())
        .await
        .expect("the first provision");
    let second = accounts
        .provision(&identity, ProvisionPolicy::default())
        .await
        .expect("the second provision");

    assert_eq!(first, second);
    assert_eq!(account_count(&pool).await, 1);
}

#[tokio::test]
async fn two_providers_may_carry_the_same_subject() {
    let Some(pool) = migrated("identity_two_providers").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let accounts = backend::accounts(&pg);
    let one = a_federated_identity("https://one.example", "shared", "one@example.com");
    let other = a_federated_identity("https://other.example", "shared", "other@example.com");

    let first = accounts
        .provision(&one, ProvisionPolicy::default())
        .await
        .expect("the first provider");
    let second = accounts
        .provision(&other, ProvisionPolicy::default())
        .await
        .expect("the second provider");

    assert_ne!(
        first, second,
        "one subject under two issuers is two identities"
    );
    assert_eq!(account_count(&pool).await, 2);
}

#[tokio::test]
async fn linking_by_email_joins_an_existing_account() {
    let Some(pool) = migrated("identity_by_email").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let accounts = backend::accounts(&pg);
    let first_provider = a_federated_identity("https://one.example", "sub-1", "person@example.com");
    let second_provider =
        a_federated_identity("https://other.example", "sub-2", "person@example.com");

    let first = accounts
        .provision(&first_provider, ProvisionPolicy::default())
        .await
        .expect("the first provider");
    let second = accounts
        .provision(
            &second_provider,
            ProvisionPolicy {
                auto_provision: false,
                link_by_email: true,
            },
        )
        .await
        .expect("the second provider joins by email");

    assert_eq!(first, second);
    assert_eq!(account_count(&pool).await, 1);
}

#[tokio::test]
async fn a_local_identity_is_the_account_it_names_and_needs_no_link() {
    let Some(pool) = migrated("identity_local").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let accounts = backend::accounts(&pg);
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'x')")
        .bind(id)
        .bind("person@example.com")
        .execute(&pool)
        .await
        .expect("an account");
    let local = an_identity(Issuer::Local, &id.to_string(), "person@example.com");

    let resolved = accounts
        .provision(&local, ProvisionPolicy::default())
        .await
        .expect("the local identity");

    assert_eq!(resolved.as_uuid(), id);
    assert_eq!(account_count(&pool).await, 1);
    assert_eq!(
        link_count(&pool).await,
        0,
        "a local account is its own identity, so it needs no link row"
    );
}

#[tokio::test]
async fn a_local_subject_that_is_not_an_account_id_is_refused() {
    let Some(pool) = migrated("identity_local_nonsense").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));

    let refused = backend::accounts(&pg)
        .provision(
            &an_identity(Issuer::Local, "not-a-uuid", "person@example.com"),
            ProvisionPolicy::default(),
        )
        .await;

    assert!(matches!(refused, Err(AccountError::NotProvisioned)));
    assert_eq!(account_count(&pool).await, 0);
}

#[tokio::test]
async fn an_unknown_identity_is_refused_when_provisioning_is_off() {
    let Some(pool) = migrated("identity_closed").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let accounts = backend::accounts(&pg);
    let identity = a_federated_identity("https://id.example", "sub-1", "stranger@example.com");

    let refused = accounts
        .provision(
            &identity,
            ProvisionPolicy {
                auto_provision: false,
                link_by_email: false,
            },
        )
        .await;

    assert!(matches!(refused, Err(AccountError::NotProvisioned)));
    assert_eq!(account_count(&pool).await, 0, "nothing was left behind");
}

async fn a_key(pool: &PgPool, org: uuid::Uuid, prefix: &str, secret: Option<&str>) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    let digest = secret.map(|plain| SecretHash::of(plain).as_bytes().to_vec());
    sqlx::query(
        "INSERT INTO api_keys (id, org_id, prefix, hash, secret_digest) \
         VALUES ($1, $2, $3, 'superseded', $4)",
    )
    .bind(id)
    .bind(org)
    .bind(prefix)
    .bind(digest)
    .execute(pool)
    .await
    .expect("a key");
    id
}

async fn last_seen(pool: &PgPool, key: uuid::Uuid) -> Option<i64> {
    sqlx::query("SELECT last_used_at AS last_used_at FROM api_keys WHERE id = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("reading the sighting")
        .get("last_used_at")
}

#[tokio::test]
async fn a_live_key_resolves_to_its_organisation() {
    let Some(pool) = migrated("keys_live").await else {
        return;
    };
    let org = seeded_org(&pool).await;
    a_key(&pool, org, "ft_live_abc", Some("the-secret")).await;
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    let resolved = directory
        .resolve(&IngestKey::from_plaintext("ft_live_abc.the-secret"))
        .await;

    assert_eq!(resolved, Ok(Scope::of(TenantId::new(org))));
}

#[tokio::test]
async fn a_wrong_secret_an_unknown_prefix_and_a_malformed_key_all_refuse() {
    let Some(pool) = migrated("keys_refused").await else {
        return;
    };
    let org = seeded_org(&pool).await;
    a_key(&pool, org, "ft_live_abc", Some("the-secret")).await;
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    for candidate in [
        "ft_live_abc.wrong-secret",
        "ft_live_zzz.the-secret",
        "ft_live_abc",
    ] {
        assert_eq!(
            directory
                .resolve(&IngestKey::from_plaintext(candidate))
                .await,
            Err(UnknownKey),
            "{candidate} should be refused"
        );
    }
}

#[tokio::test]
async fn a_revoked_key_no_longer_resolves() {
    let Some(pool) = migrated("keys_revoked").await else {
        return;
    };
    let org = seeded_org(&pool).await;
    let key = a_key(&pool, org, "ft_live_abc", Some("the-secret")).await;
    sqlx::query("UPDATE api_keys SET revoked_at = 1 WHERE id = $1")
        .bind(key)
        .execute(&pool)
        .await
        .expect("the revocation");
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    assert_eq!(
        directory
            .resolve(&IngestKey::from_plaintext("ft_live_abc.the-secret"))
            .await,
        Err(UnknownKey)
    );
}

#[tokio::test]
async fn a_key_left_without_a_digest_cannot_authenticate() {
    let Some(pool) = migrated("keys_no_digest").await else {
        return;
    };
    let org = seeded_org(&pool).await;
    a_key(&pool, org, "ft_live_abc", None).await;
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    assert_eq!(
        directory
            .resolve(&IngestKey::from_plaintext("ft_live_abc.the-secret"))
            .await,
        Err(UnknownKey)
    );
}

#[tokio::test]
async fn a_sighting_is_recorded_once_then_left_alone_for_a_while() {
    let Some(pool) = migrated("keys_sighting").await else {
        return;
    };
    let org = seeded_org(&pool).await;
    let key = a_key(&pool, org, "ft_live_abc", Some("the-secret")).await;
    let ingest = IngestKey::from_plaintext("ft_live_abc.the-secret");
    let now = 1_700_000_000;

    assert_eq!(last_seen(&pool, key).await, None);
    PostgresDirectory::new(pool.clone(), FixedClock::at(now))
        .resolve(&ingest)
        .await
        .expect("the first use");
    let first = last_seen(&pool, key).await;

    PostgresDirectory::new(pool.clone(), FixedClock::at(now + 60))
        .resolve(&ingest)
        .await
        .expect("a use inside the gap");
    let unchanged = last_seen(&pool, key).await;

    PostgresDirectory::new(pool.clone(), FixedClock::at(now + 400))
        .resolve(&ingest)
        .await
        .expect("a use past the gap");
    let moved = last_seen(&pool, key).await;

    assert!(first.is_some());
    assert_eq!(unchanged, first, "a sighting inside the gap costs no write");
    assert_ne!(moved, first, "a sighting past the gap is recorded");
}

#[tokio::test]
async fn an_issued_key_authenticates_at_once() {
    let Some(pool) = migrated("keys_issued").await else {
        return;
    };
    let org = TenantId::new(seeded_org(&pool).await);
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    let issued = directory.issue(org, None).await.expect("a key is issued");
    let resolved = directory.resolve(&issued).await;

    assert_eq!(resolved, Ok(Scope::of(org)));
    assert_eq!(directory.live_keys(org).await.expect("a count"), 1);
}

#[tokio::test]
async fn an_issued_key_carries_a_prefix_and_a_long_secret() {
    let Some(pool) = migrated("keys_shape").await else {
        return;
    };
    let org = TenantId::new(seeded_org(&pool).await);
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    let issued = directory.issue(org, None).await.expect("a key is issued");
    let (prefix, secret) = issued.split().expect("it splits");

    assert!(prefix.starts_with("ft_live_"), "{prefix}");
    assert_eq!(secret.len(), 64, "a 32 byte secret in hex");
    assert_eq!(format!("{issued:?}"), "IngestKey(redacted)");
}

#[tokio::test]
async fn two_issued_keys_never_collide() {
    let Some(pool) = migrated("keys_distinct").await else {
        return;
    };
    let org = TenantId::new(seeded_org(&pool).await);
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    let first = directory.issue(org, None).await.expect("the first");
    let second = directory.issue(org, None).await.expect("the second");

    assert_ne!(first.exposed(), second.exposed());
    assert_eq!(directory.resolve(&first).await, Ok(Scope::of(org)));
    assert_eq!(directory.resolve(&second).await, Ok(Scope::of(org)));
    assert_eq!(directory.live_keys(org).await.expect("a count"), 2);
}

#[tokio::test]
async fn revoking_a_key_stops_it_without_touching_the_others() {
    let Some(pool) = migrated("keys_revoke").await else {
        return;
    };
    let org = TenantId::new(seeded_org(&pool).await);
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));
    let doomed = directory.issue(org, None).await.expect("the first");
    let kept = directory.issue(org, None).await.expect("the second");
    let (prefix, _) = doomed.split().expect("it splits");

    let revoked = directory.revoke(prefix).await.expect("a revocation");

    assert!(revoked);
    assert_eq!(directory.resolve(&doomed).await, Err(UnknownKey));
    assert_eq!(directory.resolve(&kept).await, Ok(Scope::of(org)));
    assert_eq!(directory.live_keys(org).await.expect("a count"), 1);
}

#[tokio::test]
async fn revoking_a_prefix_nobody_issued_changes_nothing() {
    let Some(pool) = migrated("keys_revoke_absent").await else {
        return;
    };
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    assert!(!directory.revoke("ft_live_nothing").await.expect("a call"));
}

#[tokio::test]
async fn an_organisation_holds_the_projects_it_was_given() {
    let Some(pool) = migrated("projects_held").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = TenantId::new(seeded_org(&pool).await);
    let projects = backend::projects(&pg);

    let storefront = projects
        .claim(org, &a_slug("storefront"))
        .await
        .expect("one");
    projects
        .claim(org, &a_slug("payments"))
        .await
        .expect("another");

    let held = projects.of(org).await.expect("the projects read");

    assert_eq!(held.len(), 2);
    assert_eq!(
        held.iter().map(|p| &*p.name).collect::<Vec<_>>(),
        vec!["payments", "storefront"],
        "a list a person reads is ordered by name"
    );
    assert!(held.iter().any(|p| p.id == storefront));
    assert!(held.iter().all(|p| p.org == org));
}

#[tokio::test]
async fn two_organisations_may_each_hold_a_project_of_the_same_name() {
    let Some(pool) = migrated("projects_same_name").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let mine = TenantId::new(seeded_org(&pool).await);
    let theirs = TenantId::new(seeded_org(&pool).await);
    let projects = backend::projects(&pg);

    projects
        .claim(mine, &a_slug("storefront"))
        .await
        .expect("mine");
    projects
        .claim(theirs, &a_slug("storefront"))
        .await
        .expect("theirs");

    assert_eq!(projects.of(mine).await.expect("a read").len(), 1);
    assert_eq!(projects.of(theirs).await.expect("a read").len(), 1);
    assert!(
        matches!(
            projects.claim(mine, &a_slug("storefront")).await,
            Err(ProjectError::Taken(_))
        ),
        "one organisation cannot hold the same name twice"
    );
}

#[tokio::test]
async fn a_project_of_an_organisation_nobody_holds_is_refused() {
    let Some(pool) = migrated("projects_no_org").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));

    let refused = backend::projects(&pg)
        .claim(TenantId::new(Uuid::new_v4()), &a_slug("storefront"))
        .await;

    assert!(matches!(refused, Err(ProjectError::NoSuchOrg(_))));
}

#[tokio::test]
async fn the_only_project_is_named_only_when_there_is_exactly_one() {
    let Some(pool) = migrated("projects_only").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = TenantId::new(seeded_org(&pool).await);
    let projects = backend::projects(&pg);

    assert_eq!(projects.only(org).await.expect("a read"), None, "none yet");

    let first = projects
        .claim(org, &a_slug("storefront"))
        .await
        .expect("one");
    assert_eq!(projects.only(org).await.expect("a read"), Some(first));

    projects
        .claim(org, &a_slug("payments"))
        .await
        .expect("another");
    assert_eq!(
        projects.only(org).await.expect("a read"),
        None,
        "with two, the choice is the operator's"
    );
}

#[tokio::test]
async fn a_key_resolves_to_the_project_it_was_issued_for() {
    let Some(pool) = migrated("keys_project").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = TenantId::new(seeded_org(&pool).await);
    let project = backend::projects(&pg)
        .claim(org, &a_slug("storefront"))
        .await
        .expect("a project");
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    let key = directory
        .issue(org, Some(project.as_uuid()))
        .await
        .expect("a key is issued");

    assert_eq!(
        directory.resolve(&key).await,
        Ok(Scope::of(org).in_project(Some(project)))
    );
}

#[tokio::test]
async fn a_key_issued_for_no_project_resolves_to_none() {
    let Some(pool) = migrated("keys_no_project").await else {
        return;
    };
    let org = TenantId::new(seeded_org(&pool).await);
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));

    let key = directory.issue(org, None).await.expect("a key is issued");

    assert_eq!(directory.resolve(&key).await, Ok(Scope::of(org)));
}

#[tokio::test]
async fn a_postgres_directory_honours_the_directory_contract() {
    let Some(pool) = migrated("keys_contract").await else {
        return;
    };
    let org = seeded_org(&pool).await;
    a_key(&pool, org, "ft_live_abc", Some("the-secret")).await;
    let directory = PostgresDirectory::new(pool.clone(), FixedClock::at(1_700_000_000));
    let held = IngestKey::from_plaintext("ft_live_abc.the-secret");
    let absent = IngestKey::from_plaintext("ft_live_zzz.the-secret");

    testing::a_directory_resolves_the_key_it_holds(&directory, &held, TenantId::new(org)).await;
    testing::a_directory_refuses_a_key_it_does_not_hold(&directory, &absent).await;
    testing::a_directory_answers_the_same_key_the_same_way_twice(&directory, &held).await;
}

#[tokio::test]
async fn an_organisation_founded_in_a_transaction_that_fails_does_not_exist() {
    let Some(pool) = migrated("control_plane_rollback").await else {
        return;
    };
    sqlx::query(
        "INSERT INTO accounts (id, email, password_hash) \
         VALUES (gen_random_uuid(), 'founder@example.com', 'x')",
    )
    .execute(&pool)
    .await
    .expect("an account to own the organisation");
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let backend = &pg;
    let name = OrgName::try_from("Acme").expect("a name");
    let owner = Email::try_from("founder@example.com").expect("an address");

    let interrupted: Result<TenantId, ()> = UnitOfWork::transaction(
        backend,
        || (),
        Settle::OnSuccess,
        |tx| async move {
            let control_plane =
                <Postgres<FixedClock> as RepoFor<domain::ControlPlane>>::build(backend, &tx);
            control_plane
                .found(&name, &owner)
                .await
                .expect("the organisation is founded");
            Err(())
        },
    )
    .await;

    assert!(interrupted.is_err());
    let founded = committed!(pg, |control_plane: ControlPlane| control_plane
        .founded()
        .await
        .expect("the control plane answers"));
    assert_eq!(founded, None, "the failed transaction left no organisation");
}
