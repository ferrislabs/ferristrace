use ferristrace_domain::{
    AccountId, ModelPattern, ModelPrices, Price, PriceDraft, PriceId, Provider, Rates, TenantId,
};
use ferristrace_infra::FixedClock;
use ferristrace_postgres::{CONTROL_PLANE, Postgres};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

#[macro_use]
mod backend;

const NOW: u64 = 1_700_000_040;

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

async fn an_org(pool: &PgPool, name: &str, owner: &str) -> (TenantId, AccountId) {
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'chosen')")
        .bind(account)
        .bind(owner)
        .execute(pool)
        .await
        .expect("inserting an account");
    let org = backend::memberships(&backend_of(pool))
        .found(name, owner)
        .await
        .expect("the organisation is founded");
    (org, AccountId::new(account))
}

fn draft(provider: &str, model: &str, from_ms: u64, cached: Option<u64>) -> PriceDraft {
    PriceDraft {
        provider: Provider::try_from(provider).expect("a provider"),
        model: ModelPattern::try_from(model).expect("a model"),
        rates: Rates::new(2_500_000, 10_000_000, cached).expect("rates"),
        effective_from_ms: from_ms,
    }
}

async fn added(
    pg: &Postgres<FixedClock>,
    org: TenantId,
    by: AccountId,
    draft: &PriceDraft,
) -> Option<Price> {
    committed!(*pg, |prices: ModelPrices| prices
        .create(org, draft, by)
        .await
        .expect("kept"))
}

async fn listed(pg: &Postgres<FixedClock>, org: TenantId) -> Vec<Price> {
    committed!(*pg, |prices: ModelPrices| prices
        .list(org)
        .await
        .expect("a list"))
}

#[tokio::test]
async fn spec_l4_2_a_price_reads_back_with_its_rates_and_date() {
    let Some(pool) = control_plane("prices_round_trip").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, by) = an_org(&pool, "Acme", "owner@example.com").await;

    let plain = added(&pg, org, by, &draft("OpenAI", "gpt-4o", 1_000, None))
        .await
        .expect("fresh");
    let cached = added(
        &pg,
        org,
        by,
        &draft("openai", "gpt-4o*", 2_000, Some(1_250_000)),
    )
    .await
    .expect("fresh");

    let held = listed(&pg, org).await;
    assert_eq!(held, vec![plain.clone(), cached.clone()]);
    assert_eq!(plain.provider.as_str(), "openai");
    assert_eq!(plain.rates.cached_input, None);
    assert_eq!(cached.rates.cached_input, Some(1_250_000));
    assert_eq!(cached.effective_from_ms, 2_000);
}

#[tokio::test]
async fn spec_l4_2_a_model_is_priced_once_per_provider_and_effective_date() {
    let Some(pool) = control_plane("prices_unique").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, by) = an_org(&pool, "Acme", "owner@example.com").await;
    let (other, other_by) = an_org(&pool, "Globex", "boss@example.com").await;

    let first = added(&pg, org, by, &draft("openai", "gpt-4o", 1_000, None)).await;
    let again = added(&pg, org, by, &draft("OPENAI", "gpt-4o", 1_000, Some(1))).await;
    let later = added(&pg, org, by, &draft("openai", "gpt-4o", 2_000, None)).await;
    let elsewhere = added(&pg, org, by, &draft("anthropic", "gpt-4o", 1_000, None)).await;
    let theirs = added(
        &pg,
        other,
        other_by,
        &draft("openai", "gpt-4o", 1_000, None),
    )
    .await;

    assert!(first.is_some());
    assert!(again.is_none());
    assert!(later.is_some());
    assert!(elsewhere.is_some());
    assert!(theirs.is_some());
    let held = listed(&pg, org).await;
    assert_eq!(held.len(), 3);
    assert_eq!(held[0].provider.as_str(), "anthropic");
    assert_eq!(
        held.iter()
            .map(|price| price.effective_from_ms)
            .collect::<Vec<_>>(),
        vec![1_000, 2_000, 1_000]
    );
}

#[tokio::test]
async fn spec_l4_2_a_deleted_price_is_returned_once_and_gone() {
    let Some(pool) = control_plane("prices_delete").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, by) = an_org(&pool, "Acme", "owner@example.com").await;
    let price = added(&pg, org, by, &draft("openai", "gpt-4o", 1_000, None))
        .await
        .expect("fresh");

    let deleted = committed!(pg, |prices: ModelPrices| prices
        .delete(org, price.id)
        .await
        .expect("deleted"));
    let twice = committed!(pg, |prices: ModelPrices| prices
        .delete(org, price.id)
        .await
        .expect("deleted"));
    let unknown = committed!(pg, |prices: ModelPrices| prices
        .delete(org, PriceId::fresh())
        .await
        .expect("deleted"));

    assert_eq!(deleted, Some(price));
    assert_eq!(twice, None);
    assert_eq!(unknown, None);
    assert!(listed(&pg, org).await.is_empty());
}

#[tokio::test]
async fn spec_l4_2_one_organisation_never_sees_or_deletes_the_prices_of_another() {
    let Some(pool) = control_plane("prices_isolation").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (ours, by) = an_org(&pool, "Acme", "owner@example.com").await;
    let (theirs, _) = an_org(&pool, "Globex", "boss@example.com").await;
    let price = added(&pg, ours, by, &draft("openai", "gpt-4o", 1_000, None))
        .await
        .expect("fresh");

    let stolen = committed!(pg, |prices: ModelPrices| prices
        .delete(theirs, price.id)
        .await
        .expect("deleted"));

    assert_eq!(stolen, None);
    assert!(listed(&pg, theirs).await.is_empty());
    assert_eq!(listed(&pg, ours).await, vec![price]);
}

#[tokio::test]
async fn spec_l4_2_prices_leave_with_their_organisation() {
    let Some(pool) = control_plane("prices_cascade").await else {
        return;
    };
    let pg = backend_of(&pool);
    let (org, by) = an_org(&pool, "Acme", "owner@example.com").await;
    added(&pg, org, by, &draft("openai", "gpt-4o", 1_000, None)).await;

    sqlx::query("DELETE FROM orgs WHERE id = $1")
        .bind(org.as_uuid())
        .execute(&pool)
        .await
        .expect("an organisation deleted");

    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM model_prices")
        .fetch_one(&pool)
        .await
        .expect("a count");
    assert_eq!(left, 0);
}
