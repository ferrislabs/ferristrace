use ferristrace_application::testing::Inert;
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_domain::{
    DecompressedBytes, NoPhotos, ProvisionPolicy, TenantId, UsageMeter, UsageReader,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres, SELF_HOSTED_PLAN};
use ferristrace_redis::RedisMeter;
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

const NOVEMBER: u64 = 1_700_000_000;
const DECEMBER: u64 = 1_702_600_000;

mod backend;

async fn meter_at(seconds: u64) -> Option<RedisMeter<FixedClock>> {
    namespaced_meter_at(seconds, "shared").await
}

async fn namespaced_meter_at(seconds: u64, namespace: &str) -> Option<RedisMeter<FixedClock>> {
    let url = backend::redis_url()?;
    Some(
        RedisMeter::connect(&url, FixedClock::at(seconds))
            .await
            .expect("connecting to the usage counter store")
            .with_prefix(format!("test:{namespace}")),
    )
}

#[derive(Debug)]
struct Wired;

impl Stack for Wired {
    type Repos = Postgres<FixedClock>;
    type Clock = FixedClock;
    type Hasher = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = Inert;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = RedisMeter<FixedClock>;
    type Oidc = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
}

async fn driven(pool: &PgPool, seconds: u64, namespace: &str) -> Option<Ferristrace<Wired>> {
    let meter = namespaced_meter_at(seconds, namespace).await?;
    Some(Ferristrace::new(
        Postgres::new(pool.clone(), FixedClock::at(seconds)),
        Adapters {
            clock: FixedClock::at(seconds),
            hasher: Inert::default(),
            mailer: Inert::default(),
            photos: NoPhotos,
            meter,
            span_store: None,
            issue_store: None,
            log_store: None,
            metric_store: None,
            oidc: None,
            provision: ProvisionPolicy::default(),
        },
    ))
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

#[tokio::test]
async fn a_recorded_payload_shows_up_in_the_monthly_counter() {
    let Some(meter) = meter_at(NOVEMBER).await else {
        return;
    };
    let tenant = TenantId::new(Uuid::new_v4());

    meter
        .record_ingest(tenant, DecompressedBytes::new(400))
        .await;
    meter
        .record_ingest(tenant, DecompressedBytes::new(600))
        .await;

    assert_eq!(meter.used(tenant).await, DecompressedBytes::new(1000));
}

#[tokio::test]
async fn an_unseen_tenant_has_used_nothing() {
    let Some(meter) = meter_at(NOVEMBER).await else {
        return;
    };

    assert_eq!(
        meter.used(TenantId::new(Uuid::new_v4())).await,
        DecompressedBytes::new(0)
    );
}

#[tokio::test]
async fn two_tenants_do_not_share_a_counter() {
    let Some(meter) = meter_at(NOVEMBER).await else {
        return;
    };
    let first = TenantId::new(Uuid::new_v4());
    let second = TenantId::new(Uuid::new_v4());

    meter
        .record_ingest(first, DecompressedBytes::new(100))
        .await;

    assert_eq!(meter.used(first).await, DecompressedBytes::new(100));
    assert_eq!(meter.used(second).await, DecompressedBytes::new(0));
}

#[tokio::test]
async fn a_new_month_starts_the_counter_again() {
    let Some(november) = meter_at(NOVEMBER).await else {
        return;
    };
    let december = meter_at(DECEMBER).await.expect("the same store");
    let tenant = TenantId::new(Uuid::new_v4());

    november
        .record_ingest(tenant, DecompressedBytes::new(900))
        .await;

    assert_eq!(november.used(tenant).await, DecompressedBytes::new(900));
    assert_eq!(december.used(tenant).await, DecompressedBytes::new(0));
}

#[tokio::test]
async fn rehydration_seeds_the_counter_from_the_billed_hours() {
    let Some(pool) = control_plane("counter_rehydrate").await else {
        return;
    };
    let Some(meter) = namespaced_meter_at(NOVEMBER, "rehydrate").await else {
        return;
    };
    let tenant = seeded_org(&pool).await;
    billed(&pool, tenant, 1_698_800_000_000_000_000, 700).await;
    billed(&pool, tenant, 1_698_900_000_000_000_000, 300).await;

    let seeded = driven(&pool, NOVEMBER, "rehydrate")
        .await
        .expect("the same store")
        .rehydrate_usage()
        .await
        .expect("rehydration runs");

    assert_eq!(seeded, 1);
    assert_eq!(meter.used(tenant).await, DecompressedBytes::new(1000));
}

#[tokio::test]
async fn rehydration_leaves_a_live_counter_alone() {
    let Some(pool) = control_plane("counter_live").await else {
        return;
    };
    let Some(meter) = namespaced_meter_at(NOVEMBER, "live").await else {
        return;
    };
    let tenant = seeded_org(&pool).await;
    billed(&pool, tenant, 1_698_800_000_000_000_000, 700).await;
    meter
        .record_ingest(tenant, DecompressedBytes::new(50))
        .await;

    let seeded = driven(&pool, NOVEMBER, "live")
        .await
        .expect("the same store")
        .rehydrate_usage()
        .await
        .expect("rehydration runs");

    assert_eq!(seeded, 0, "a counter already in flight must not be reset");
    assert_eq!(meter.used(tenant).await, DecompressedBytes::new(50));
}

#[tokio::test]
async fn hours_before_the_month_are_not_rehydrated() {
    let Some(pool) = control_plane("counter_earlier").await else {
        return;
    };
    let Some(meter) = namespaced_meter_at(NOVEMBER, "earlier").await else {
        return;
    };
    let tenant = seeded_org(&pool).await;
    billed(&pool, tenant, 1_690_000_000_000_000_000, 5_000).await;

    driven(&pool, NOVEMBER, "earlier")
        .await
        .expect("the same store")
        .rehydrate_usage()
        .await
        .expect("rehydration runs");

    assert_eq!(meter.used(tenant).await, DecompressedBytes::new(0));
}

async fn billed(pool: &PgPool, tenant: TenantId, hour_start: i64, bytes: i64) {
    sqlx::query(
        "INSERT INTO usage_hourly (org_id, hour_start, ingested_bytes) VALUES ($1, $2, $3)",
    )
    .bind(tenant.as_uuid())
    .bind(hour_start)
    .bind(bytes)
    .execute(pool)
    .await
    .expect("a billed hour");
}

async fn seeded_org(pool: &PgPool) -> TenantId {
    let plan: Uuid = sqlx::query("SELECT id AS id FROM plans WHERE slug = $1")
        .bind(SELF_HOSTED_PLAN)
        .fetch_one(pool)
        .await
        .expect("the default plan")
        .get("id");
    let org = Uuid::new_v4();
    sqlx::query("INSERT INTO orgs (id, name, plan_id) VALUES ($1, 'Acme', $2)")
        .bind(org)
        .bind(plan)
        .execute(pool)
        .await
        .expect("an org");
    TenantId::new(org)
}

async fn billed_hours(pool: &PgPool, tenant: TenantId) -> Vec<(i64, i64)> {
    sqlx::query(
        "SELECT hour_start AS hour_start, ingested_bytes AS ingested_bytes \
         FROM usage_hourly WHERE org_id = $1 ORDER BY hour_start",
    )
    .bind(tenant.as_uuid())
    .fetch_all(pool)
    .await
    .expect("reading the billed hours")
    .into_iter()
    .map(|row| (row.get("hour_start"), row.get("ingested_bytes")))
    .collect()
}

#[tokio::test]
async fn a_finished_hour_moves_into_the_billing_table() {
    let Some(pool) = control_plane("counter_rollup").await else {
        return;
    };
    let Some(earlier) = namespaced_meter_at(NOVEMBER, "rollup").await else {
        return;
    };
    let later = driven(&pool, NOVEMBER + 3600, "rollup")
        .await
        .expect("the same store");
    let tenant = seeded_org(&pool).await;

    earlier
        .record_ingest(tenant, DecompressedBytes::new(1500))
        .await;
    let rolled = later.roll_up_usage().await.expect("the rollup runs");

    assert_eq!(rolled, 1);
    assert_eq!(
        billed_hours(&pool, tenant).await,
        vec![(1_699_999_200_000_000_000, 1500)]
    );
}

#[tokio::test]
async fn rolling_the_same_hour_twice_does_not_double_the_bill() {
    let Some(pool) = control_plane("counter_twice").await else {
        return;
    };
    let Some(earlier) = namespaced_meter_at(NOVEMBER, "twice").await else {
        return;
    };
    let later = driven(&pool, NOVEMBER + 3600, "twice")
        .await
        .expect("the same store");
    let tenant = seeded_org(&pool).await;

    earlier
        .record_ingest(tenant, DecompressedBytes::new(800))
        .await;
    let first = later.roll_up_usage().await.expect("the first rollup");
    let second = later.roll_up_usage().await.expect("the second rollup");

    assert_eq!((first, second), (1, 0));
    let hours = billed_hours(&pool, tenant).await;
    assert_eq!(hours.len(), 1);
    assert_eq!(hours[0].1, 800);
}

#[tokio::test]
async fn the_hour_in_progress_is_left_to_finish() {
    let Some(pool) = control_plane("counter_current").await else {
        return;
    };
    let Some(meter) = namespaced_meter_at(NOVEMBER, "current").await else {
        return;
    };
    let tenant = seeded_org(&pool).await;

    meter
        .record_ingest(tenant, DecompressedBytes::new(400))
        .await;
    let rolled = driven(&pool, NOVEMBER, "current")
        .await
        .expect("the same store")
        .roll_up_usage()
        .await
        .expect("the rollup runs");

    assert_eq!(rolled, 0);
    assert!(billed_hours(&pool, tenant).await.is_empty());
}

#[tokio::test]
async fn a_rollup_leaves_the_monthly_counter_standing() {
    let Some(pool) = control_plane("counter_month_kept").await else {
        return;
    };
    let Some(earlier) = namespaced_meter_at(NOVEMBER, "month_kept").await else {
        return;
    };
    let later = driven(&pool, NOVEMBER + 3600, "month_kept")
        .await
        .expect("the same store");
    let tenant = seeded_org(&pool).await;

    earlier
        .record_ingest(tenant, DecompressedBytes::new(1200))
        .await;
    later.roll_up_usage().await.expect("the rollup runs");

    assert_eq!(earlier.used(tenant).await, DecompressedBytes::new(1200));
}
