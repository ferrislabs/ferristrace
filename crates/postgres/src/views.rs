use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, Creation, ExplorerViews, NewView, Saving, StoreError, TenantId, View, ViewChange,
    ViewId, ViewSignal, Visibility,
};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

macro_rules! columns {
    () => {
        "id, signal, name, visibility, definition::text AS definition, \
         created_by, created_at, updated_at"
    };
}

#[repository(domain = Views, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresViews {
    tx: SharedTx,
}

impl PostgresViews {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl ExplorerViews for PostgresViews {
    async fn visible(
        &self,
        org: TenantId,
        signal: ViewSignal,
        to: AccountId,
    ) -> Result<Vec<View>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM explorer_views \
             WHERE org_id = $1 AND signal = $2 \
               AND (visibility = 'org' OR created_by = $3) \
             ORDER BY lower(name), name, id"
        ))
        .bind(org.as_uuid())
        .bind(signal.as_str())
        .bind(to.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "read the views"))?;

        rows.iter().map(read_view).collect()
    }

    async fn find(&self, org: TenantId, id: ViewId) -> Result<Option<View>, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM explorer_views WHERE org_id = $1 AND id = $2"
        ))
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "read a view"))?;

        row.as_ref().map(read_view).transpose()
    }

    async fn create(
        &self,
        org: TenantId,
        by: AccountId,
        view: &NewView,
    ) -> Result<Creation, StoreError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(concat!(
            "INSERT INTO explorer_views \
               (id, org_id, signal, name, visibility, definition, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7) \
             ON CONFLICT DO NOTHING RETURNING ",
            columns!()
        ))
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(view.signal.as_str())
        .bind(view.name.as_str())
        .bind(view.visibility.as_str())
        .bind(view.definition.as_str())
        .bind(by.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "create a view"))?;

        Ok(match row {
            Some(row) => Creation::Made(read_view(&row)?),
            None => Creation::NameTaken,
        })
    }

    async fn change(
        &self,
        org: TenantId,
        id: ViewId,
        change: &ViewChange,
    ) -> Result<Saving, StoreError> {
        let mut tx = self.tx.lock().await;
        let visibility = change.visibility.map(Visibility::as_str);

        let clash = sqlx::query(
            "SELECT 1 AS one FROM explorer_views other \
             JOIN explorer_views mine ON mine.id = $2 AND mine.org_id = $1 \
             WHERE other.org_id = $1 AND other.id <> mine.id AND other.signal = mine.signal \
               AND other.name = $3 \
               AND other.visibility = COALESCE($4, mine.visibility) \
               AND (other.visibility = 'org' OR other.created_by IS NOT DISTINCT FROM mine.created_by)",
        )
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .bind(change.name.as_str())
        .bind(visibility)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "look a view name up"))?;
        if clash.is_some() {
            return Ok(Saving::NameTaken);
        }

        let done = sqlx::query(
            "UPDATE explorer_views SET name = $3, definition = $4::jsonb, \
               visibility = COALESCE($5, visibility), \
               updated_at = (extract(epoch FROM clock_timestamp()) * 1000000000)::bigint \
             WHERE org_id = $1 AND id = $2",
        )
        .bind(org.as_uuid())
        .bind(id.as_uuid())
        .bind(change.name.as_str())
        .bind(change.definition.as_str())
        .bind(visibility)
        .execute(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "change a view"))?;

        Ok(if done.rows_affected() > 0 {
            Saving::Saved
        } else {
            Saving::Missing
        })
    }

    async fn delete(&self, org: TenantId, id: ViewId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("DELETE FROM explorer_views WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(id.as_uuid())
            .execute(&mut **tx)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|error| unreachable(&error, "delete a view"))
    }
}

fn read_view(row: &PgRow) -> Result<View, StoreError> {
    let signal: String = row.get("signal");
    let visibility: String = row.get("visibility");
    Ok(View {
        id: ViewId::new(row.get("id")),
        signal: ViewSignal::try_from(signal.as_str()).map_err(|()| unreadable("signal"))?,
        name: row.get::<String, _>("name").into(),
        visibility: Visibility::try_from(visibility.as_str())
            .map_err(|()| unreadable("visibility"))?,
        definition: row.get::<String, _>("definition").into(),
        created_by: row.get::<Option<Uuid>, _>("created_by").map(AccountId::new),
        created_at_unix_nanos: u64::try_from(row.get::<i64, _>("created_at")).unwrap_or_default(),
        updated_at_unix_nanos: u64::try_from(row.get::<i64, _>("updated_at")).unwrap_or_default(),
    })
}

fn unreadable(column: &str) -> StoreError {
    tracing::error!(
        column,
        "a stored view holds a value this version does not know"
    );
    StoreError::Unreachable
}

fn unreachable(error: &sqlx::Error, what: &str) -> StoreError {
    tracing::error!(%error, "the control plane refused to {what}");
    StoreError::Unreachable
}
