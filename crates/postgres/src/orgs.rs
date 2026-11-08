use ferristrace_application::repository;
use ferristrace_domain::{
    OrgName, OrgTimezone, Organisation, Orgs, Portrait, RetentionDays, StoreError, TenantId,
};
use sqlx::Row;

use crate::accounts::read_portrait;
use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = Orgs, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresOrgs {
    tx: SharedTx,
}

impl PostgresOrgs {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl Orgs for PostgresOrgs {
    async fn only(&self, org: TenantId) -> Result<Option<Organisation>, StoreError> {
        let mut tx = self.tx.lock().await;
        let found = sqlx::query(
            "SELECT o.name AS name, p.name AS plan, o.created_at AS created_at, \
             o.logo_token AS photo_token, o.logo_type AS photo_type, \
             o.retention_days AS retention_days, p.retention_days AS retention_cap_days, \
             o.timezone AS timezone \
             FROM orgs o JOIN plans p ON p.id = o.plan_id \
             WHERE o.id = $1",
        )
        .bind(org.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to read an organisation");
            StoreError::Unreachable
        })?;

        Ok(found.map(|row| Organisation {
            id: org,
            name: row.get::<String, _>("name").into(),
            plan: row.get::<String, _>("plan").into(),
            created_at_unix_nanos: u64::try_from(row.get::<i64, _>("created_at"))
                .unwrap_or_default(),
            logo: read_portrait(&row),
            retention: days(&row, "retention_days"),
            retention_cap: days(&row, "retention_cap_days"),
            timezone: OrgTimezone::try_from(row.get::<String, _>("timezone").as_str())
                .unwrap_or_default(),
        }))
    }

    async fn rename(&self, org: TenantId, name: &OrgName) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("UPDATE orgs SET name = $2 WHERE id = $1")
            .bind(org.as_uuid())
            .bind(name.as_str())
            .execute(&mut **tx)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to rename an organisation");
                StoreError::Unreachable
            })?;
        Ok(())
    }

    async fn disband(&self, org: TenantId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        let done = sqlx::query("DELETE FROM orgs WHERE id = $1")
            .bind(org.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to disband an organisation");
                StoreError::Unreachable
            })?;
        Ok(done.rows_affected() > 0)
    }

    async fn set_retention(
        &self,
        org: TenantId,
        retention: Option<RetentionDays>,
    ) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("UPDATE orgs SET retention_days = $2 WHERE id = $1")
            .bind(org.as_uuid())
            .bind(retention.map(|days| i32::try_from(days.get()).unwrap_or(i32::MAX)))
            .execute(&mut **tx)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to keep a retention");
                StoreError::Unreachable
            })?;
        Ok(())
    }

    async fn set_timezone(&self, org: TenantId, timezone: &OrgTimezone) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("UPDATE orgs SET timezone = $2 WHERE id = $1")
            .bind(org.as_uuid())
            .bind(timezone.as_str())
            .execute(&mut **tx)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to keep a time zone");
                StoreError::Unreachable
            })?;
        Ok(())
    }

    async fn set_logo(&self, org: TenantId, logo: Option<&Portrait>) -> Result<(), StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("UPDATE orgs SET logo_token = $2, logo_type = $3 WHERE id = $1")
            .bind(org.as_uuid())
            .bind(logo.map(|kept| kept.token.as_ref()))
            .bind(logo.map(|kept| kept.kind.as_str()))
            .execute(&mut **tx)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to keep a logo");
                StoreError::Unreachable
            })?;
        Ok(())
    }
}

fn days(row: &sqlx::postgres::PgRow, column: &str) -> Option<RetentionDays> {
    row.get::<Option<i32>, _>(column)
        .map(|days| RetentionDays::saturating(u32::try_from(days).unwrap_or_default()))
}
