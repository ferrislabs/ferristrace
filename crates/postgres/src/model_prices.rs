use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, ModelPattern, ModelPrices, Price, PriceDraft, PriceId, Provider, Rates, StoreError,
    TenantId,
};
use sqlx::Row;
use sqlx::postgres::PgRow;

use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = ModelPrices, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresModelPrices {
    tx: SharedTx,
}

impl PostgresModelPrices {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

fn unreachable(doing: &'static str) -> impl FnOnce(sqlx::Error) -> StoreError {
    move |error| {
        tracing::error!(%error, "the control plane refused to {doing}");
        StoreError::Unreachable
    }
}

fn held(what: &'static str) -> StoreError {
    tracing::error!("the control plane holds a model price with an unreadable {what}");
    StoreError::Unreachable
}

fn unsigned(row: &PgRow, column: &str) -> Result<u64, StoreError> {
    u64::try_from(row.get::<i64, _>(column)).map_err(|_| held("number"))
}

fn read_price(row: &PgRow) -> Result<Price, StoreError> {
    let cached = row
        .get::<Option<i64>, _>("cached_input_micros")
        .map(|rate| u64::try_from(rate).map_err(|_| held("rate")))
        .transpose()?;
    Ok(Price {
        id: PriceId::new(row.get("id")),
        provider: Provider::try_from(row.get::<&str, _>("provider"))
            .map_err(|_| held("provider"))?,
        model: ModelPattern::try_from(row.get::<&str, _>("model")).map_err(|_| held("model"))?,
        rates: Rates::new(
            unsigned(row, "input_micros")?,
            unsigned(row, "output_micros")?,
            cached,
        )
        .map_err(|_| held("rate"))?,
        effective_from_ms: unsigned(row, "effective_from_ms")?,
    })
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl ModelPrices for PostgresModelPrices {
    async fn list(&self, org: TenantId) -> Result<Vec<Price>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id, provider, model, input_micros, output_micros, cached_input_micros, \
             effective_from_ms FROM model_prices WHERE org_id = $1 \
             ORDER BY provider, model, effective_from_ms DESC, id",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(unreachable("list the model prices"))?;
        rows.iter().map(read_price).collect()
    }

    async fn create(
        &self,
        org: TenantId,
        draft: &PriceDraft,
        by: AccountId,
    ) -> Result<Option<Price>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "INSERT INTO model_prices \
             (id, org_id, provider, model, input_micros, output_micros, cached_input_micros, \
              effective_from_ms, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             ON CONFLICT (org_id, provider, model, effective_from_ms) DO NOTHING \
             RETURNING id, provider, model, input_micros, output_micros, cached_input_micros, \
             effective_from_ms",
        )
        .bind(PriceId::fresh().as_uuid())
        .bind(org.as_uuid())
        .bind(draft.provider.as_str())
        .bind(draft.model.as_str())
        .bind(signed(draft.rates.input))
        .bind(signed(draft.rates.output))
        .bind(draft.rates.cached_input.map(signed))
        .bind(signed(draft.effective_from_ms))
        .bind(by.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("create a model price"))?;
        row.as_ref().map(read_price).transpose()
    }

    async fn delete(&self, org: TenantId, id: PriceId) -> Result<Option<Price>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "DELETE FROM model_prices WHERE org_id = $1 AND id = $2 \
             RETURNING id, provider, model, input_micros, output_micros, cached_input_micros, \
             effective_from_ms",
        )
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(unreachable("delete a model price"))?;
        row.as_ref().map(read_price).transpose()
    }
}
