use ferristrace_domain::{OrgName, Organisation, Orgs, Portrait, ReadError, TenantId};
use sqlx::{PgPool, Row};

use crate::accounts::read_portrait;

#[derive(Debug, Clone)]
pub struct PostgresOrgs {
    pool: PgPool,
}

impl PostgresOrgs {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl Orgs for PostgresOrgs {
    async fn only(&self, org: TenantId) -> Result<Option<Organisation>, ReadError> {
        let found = sqlx::query(
            "SELECT o.name AS name, p.name AS plan, o.created_at AS created_at, \
             o.logo_token AS photo_token, o.logo_type AS photo_type \
             FROM orgs o JOIN plans p ON p.id = o.plan_id \
             WHERE o.id = $1",
        )
        .bind(org.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to read an organisation");
            ReadError::StoreUnreachable
        })?;

        Ok(found.map(|row| Organisation {
            id: org,
            name: row.get::<String, _>("name").into(),
            plan: row.get::<String, _>("plan").into(),
            created_at_unix_nanos: u64::try_from(row.get::<i64, _>("created_at"))
                .unwrap_or_default(),
            logo: read_portrait(&row),
        }))
    }

    async fn rename(&self, org: TenantId, name: &OrgName) -> Result<(), ReadError> {
        sqlx::query("UPDATE orgs SET name = $2 WHERE id = $1")
            .bind(org.as_uuid())
            .bind(name.as_str())
            .execute(&self.pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to rename an organisation");
                ReadError::StoreUnreachable
            })?;
        Ok(())
    }

    async fn set_logo(&self, org: TenantId, logo: Option<&Portrait>) -> Result<(), ReadError> {
        sqlx::query("UPDATE orgs SET logo_token = $2, logo_type = $3 WHERE id = $1")
            .bind(org.as_uuid())
            .bind(logo.map(|kept| kept.token.as_ref()))
            .bind(logo.map(|kept| kept.kind.as_str()))
            .execute(&self.pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to keep a logo");
                ReadError::StoreUnreachable
            })?;
        Ok(())
    }
}
