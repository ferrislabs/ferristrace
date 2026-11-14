use ferristrace_domain::telemetry::{BillingHour, BillingMonth, HourTotal, Split, UsageLedger};
use ferristrace_domain::{DecompressedBytes, TenantId, UnixSeconds, UsageHistory, Window};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

const AN_HOUR: u64 = 3_600_000_000_000;
const A_GIGABYTE: u64 = 1024 * 1024 * 1024;

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
    sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
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

async fn capped(pool: &PgPool, org: TenantId, gigabytes: i32) {
    let plan = Uuid::new_v4();
    sqlx::query("INSERT INTO plans (id, slug, name, ingest_gb) VALUES ($1, $2, 'Capped', $3)")
        .bind(plan)
        .bind(format!("capped-{plan}"))
        .bind(gigabytes)
        .execute(pool)
        .await
        .expect("inserting a plan");
    sqlx::query("UPDATE orgs SET plan_id = $2 WHERE id = $1")
        .bind(org.as_uuid())
        .bind(plan)
        .execute(pool)
        .await
        .expect("moving the organisation onto it");
}

async fn an_hour(pool: &PgPool, org: TenantId, start: u64, ingested: i64) {
    sqlx::query(
        "INSERT INTO usage_hourly (org_id, hour_start, ingested_bytes) VALUES ($1, $2, $3)",
    )
    .bind(org.as_uuid())
    .bind(i64::try_from(start).expect("an hour"))
    .bind(ingested)
    .execute(pool)
    .await
    .expect("inserting an hour");
}

fn a_window(from: u64, to: u64) -> Window {
    Window::new(from, to).expect("a window")
}

#[tokio::test]
async fn the_hours_of_a_window_come_back_in_order() {
    let Some(pool) = control_plane("usage_hours").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    for (hour, bytes) in [(3, 300), (1, 100), (2, 200)] {
        an_hour(&pool, org, AN_HOUR * hour, bytes).await;
    }

    let read = backend::usage(&pg)
        .hours(org, a_window(AN_HOUR, AN_HOUR * 4))
        .await
        .expect("the hours are read");

    assert_eq!(
        read.iter()
            .map(|one| one.ingested_bytes)
            .collect::<Vec<_>>(),
        vec![100, 200, 300]
    );
    assert_eq!(read[0].started_at_unix_nanos, AN_HOUR);
}

#[tokio::test]
async fn the_hour_a_window_ends_on_belongs_to_the_next_one() {
    let Some(pool) = control_plane("usage_half_open").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    an_hour(&pool, org, AN_HOUR, 100).await;
    an_hour(&pool, org, AN_HOUR * 2, 200).await;
    let usage = backend::usage(&pg);

    let first = usage
        .hours(org, a_window(AN_HOUR, AN_HOUR * 2))
        .await
        .expect("the first window");
    let second = usage
        .hours(org, a_window(AN_HOUR * 2, AN_HOUR * 3))
        .await
        .expect("the second window");

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].ingested_bytes, 100);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].ingested_bytes, 200);
}

#[tokio::test]
async fn the_hours_of_another_organisation_are_not_read() {
    let Some(pool) = control_plane("usage_theirs").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let mine = an_org(&pool, "mine@example.com").await;
    let theirs = an_org(&pool, "theirs@example.com").await;
    an_hour(&pool, mine, AN_HOUR, 100).await;
    an_hour(&pool, theirs, AN_HOUR, 900).await;

    let read = backend::usage(&pg)
        .hours(mine, a_window(AN_HOUR, AN_HOUR * 2))
        .await
        .expect("the hours are read");

    assert_eq!(read.len(), 1);
    assert_eq!(read[0].ingested_bytes, 100);
}

#[tokio::test]
async fn a_window_nothing_arrived_in_reads_as_empty_rather_than_as_an_error() {
    let Some(pool) = control_plane("usage_quiet").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;

    let read = backend::usage(&pg)
        .hours(org, a_window(AN_HOUR, AN_HOUR * 100))
        .await
        .expect("the hours are read");

    assert!(read.is_empty());
}

#[tokio::test]
async fn what_a_plan_allows_is_read_in_binary_gigabytes() {
    let Some(pool) = control_plane("usage_capped").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    capped(&pool, org, 5).await;

    let included = backend::usage(&pg)
        .included(org)
        .await
        .expect("the plan is read");

    assert_eq!(
        included,
        Some(DecompressedBytes::new(5 * A_GIGABYTE)),
        "decimal gigabytes would read as a ceiling the quota does not use"
    );
}

#[tokio::test]
async fn a_plan_with_no_ceiling_includes_nothing() {
    let Some(pool) = control_plane("usage_unlimited").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;

    let included = backend::usage(&pg)
        .included(org)
        .await
        .expect("the plan is read");

    assert_eq!(
        included, None,
        "the seeded self-hosted plan leaves every limit open"
    );
}

#[tokio::test]
async fn an_organisation_nothing_carries_includes_nothing() {
    let Some(pool) = control_plane("usage_no_org").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));

    let included = backend::usage(&pg)
        .included(TenantId::new(Uuid::new_v4()))
        .await
        .expect("the control plane answers");

    assert_eq!(included, None);
}

#[tokio::test]
async fn deleting_an_organisation_takes_its_usage_with_it() {
    let Some(pool) = control_plane("usage_cascade").await else {
        return;
    };
    let org = an_org(&pool, "owner@example.com").await;
    an_hour(&pool, org, AN_HOUR, 100).await;

    sqlx::query("DELETE FROM orgs WHERE id = $1")
        .bind(org.as_uuid())
        .execute(&pool)
        .await
        .expect("deleting the organisation");

    let left: i64 = sqlx::query("SELECT count(*) AS value FROM usage_hourly")
        .fetch_one(&pool)
        .await
        .expect("counting the hours")
        .get("value");
    assert_eq!(left, 0);
}

fn an_hour_total(tenant: TenantId, at: u64, bytes: u64) -> HourTotal {
    HourTotal {
        tenant,
        hour: BillingHour::containing(UnixSeconds::new(at)),
        ingested: DecompressedBytes::new(bytes),
        split: None,
    }
}

#[tokio::test]
async fn a_rolled_hour_is_written_once_and_never_doubled() {
    let Some(pool) = control_plane("usage_rolled").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let hours = [an_hour_total(org, 1_699_999_200, 800)];

    let first = committed!(pg, |usage: UsageHistory| usage
        .record_hours(&hours)
        .await
        .expect("the hour is written"));
    let second = committed!(pg, |usage: UsageHistory| usage
        .record_hours(&hours)
        .await
        .expect("the hour is written again"));

    assert_eq!((first, second), (1, 0));
    let kept: i64 = sqlx::query("SELECT ingested_bytes AS value FROM usage_hourly")
        .fetch_one(&pool)
        .await
        .expect("reading the hour")
        .get("value");
    assert_eq!(kept, 800);
}

#[tokio::test]
async fn an_hour_of_an_unknown_organisation_is_not_written() {
    let Some(pool) = control_plane("usage_rolled_unknown").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let stranger = TenantId::new(Uuid::new_v4());
    let hours = [an_hour_total(stranger, 1_699_999_200, 800)];

    let written = committed!(pg, |usage: UsageHistory| usage
        .record_hours(&hours)
        .await
        .expect("the control plane answers"));

    assert_eq!(written, 0);
}

#[tokio::test]
async fn the_billed_hours_of_the_month_are_summed_per_organisation() {
    let Some(pool) = control_plane("usage_billed").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    an_hour(&pool, org, 1_698_800_000_000_000_000, 700).await;
    an_hour(&pool, org, 1_698_900_000_000_000_000, 300).await;
    an_hour(&pool, org, 1_690_000_000_000_000_000, 5_000).await;
    let november = BillingMonth::containing(UnixSeconds::new(1_700_000_000));

    let billed = committed!(pg, |usage: UsageHistory| usage
        .billed_since(november)
        .await
        .expect("the billed hours are read"));

    assert_eq!(billed.len(), 1);
    assert_eq!(billed[0].tenant, org);
    assert_eq!(billed[0].ingested, DecompressedBytes::new(1000));
}

#[tokio::test]
async fn a_split_hour_reads_back_with_its_split_and_an_old_one_without() {
    let Some(pool) = control_plane("usage_split").await else {
        return;
    };
    let pg = Postgres::new(pool.clone(), FixedClock::at(1_700_000_000));
    let org = an_org(&pool, "owner@example.com").await;
    let mut split = an_hour_total(org, 1_699_999_200, 800);
    split.split = Some(Split {
        spans_bytes: 500,
        logs_bytes: 300,
        metrics_points: 12,
    });
    let unsplit = an_hour_total(org, 1_699_995_600, 90);
    committed!(pg, |usage: UsageHistory| usage
        .record_hours(&[split, unsplit])
        .await
        .expect("rolling the hours"));

    let hours = committed!(pg, |usage: UsageHistory| usage
        .hours(org, Window::new(0, u64::MAX / 2).expect("a window"))
        .await
        .expect("reading the hours"));

    assert_eq!(hours.len(), 2);
    assert_eq!(hours[0].split, None);
    assert_eq!(
        hours[1].split,
        Some(Split {
            spans_bytes: 500,
            logs_bytes: 300,
            metrics_points: 12,
        })
    );
}
