use ferristrace_domain::{AccountId, Membership, Memberships, OrgRole, TenantId, WhoamiError};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::postgres::SELF_HOSTED_PLAN;

#[derive(Debug, thiserror::Error)]
pub enum MembershipError {
    #[error("the control plane refused the membership query")]
    ControlPlane(#[from] sqlx::Error),
    #[error("no account carries the address {0}")]
    NoSuchAccount(String),
    #[error("this control plane holds no {0} plan")]
    NoSuchPlan(&'static str),
}

#[derive(Debug, Clone)]
pub struct PostgresMemberships {
    pool: PgPool,
}

impl PostgresMemberships {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn founded(&self) -> Result<Option<TenantId>, MembershipError> {
        let found = sqlx::query("SELECT id AS id FROM orgs ORDER BY created_at LIMIT 1")
            .fetch_optional(&self.pool)
            .await?;
        Ok(found.map(|row| TenantId::new(row.get("id"))))
    }

    pub async fn found(&self, name: &str, owner: &str) -> Result<TenantId, MembershipError> {
        let mut transaction = self.pool.begin().await?;

        let account: Uuid = sqlx::query("SELECT id AS id FROM accounts WHERE email = $1")
            .bind(owner)
            .fetch_optional(&mut *transaction)
            .await?
            .map(|row| row.get("id"))
            .ok_or_else(|| MembershipError::NoSuchAccount(owner.to_owned()))?;

        let plan: Uuid = sqlx::query("SELECT id AS id FROM plans WHERE slug = $1")
            .bind(SELF_HOSTED_PLAN)
            .fetch_optional(&mut *transaction)
            .await?
            .map(|row| row.get("id"))
            .ok_or_else(|| MembershipError::NoSuchPlan(SELF_HOSTED_PLAN))?;

        let org = Uuid::new_v4();
        sqlx::query("INSERT INTO orgs (id, name, plan_id) VALUES ($1, $2, $3)")
            .bind(org)
            .bind(name)
            .bind(plan)
            .execute(&mut *transaction)
            .await?;

        grant_in(&mut *transaction, org, account, OrgRole::Owner).await?;
        transaction.commit().await?;

        Ok(TenantId::new(org))
    }

    pub async fn grant(
        &self,
        org: TenantId,
        member: &str,
        role: OrgRole,
    ) -> Result<(), MembershipError> {
        let account: Uuid = sqlx::query("SELECT id AS id FROM accounts WHERE email = $1")
            .bind(member)
            .fetch_optional(&self.pool)
            .await?
            .map(|row| row.get("id"))
            .ok_or_else(|| MembershipError::NoSuchAccount(member.to_owned()))?;

        grant_in(&self.pool, org.as_uuid(), account, role).await?;

        Ok(())
    }
}

impl Memberships for PostgresMemberships {
    async fn of(&self, account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        let rows = sqlx::query(
            "SELECT m.org_id AS org_id, o.name AS name, m.role::text AS role \
             FROM org_members m JOIN orgs o ON o.id = m.org_id \
             WHERE m.account_id = $1 ORDER BY o.name, m.org_id",
        )
        .bind(account.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to read memberships");
            WhoamiError::StoreUnreachable
        })?;

        rows.into_iter()
            .map(|row| {
                let spelling: String = row.get("role");
                read_role(&spelling)
                    .map(|role| Membership {
                        org: TenantId::new(row.get("org_id")),
                        name: row.get::<String, _>("name").into(),
                        role,
                    })
                    .ok_or_else(|| {
                        tracing::error!(role = %spelling, "the control plane holds an unknown role");
                        WhoamiError::StoreUnreachable
                    })
            })
            .collect()
    }
}

async fn grant_in<'c, E>(
    executor: E,
    org: Uuid,
    account: Uuid,
    role: OrgRole,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    sqlx::query(
        "INSERT INTO org_members (org_id, account_id, role) VALUES ($1, $2, $3::org_role) \
         ON CONFLICT (org_id, account_id) DO UPDATE SET role = $3::org_role",
    )
    .bind(org)
    .bind(account)
    .bind(role.as_str())
    .execute(executor)
    .await
    .map(|_| ())
}

pub(crate) fn read_role(spelling: &str) -> Option<OrgRole> {
    match spelling {
        "owner" => Some(OrgRole::Owner),
        "admin" => Some(OrgRole::Admin),
        "member" => Some(OrgRole::Member),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::OrgRole;

    use super::read_role;

    #[test]
    fn every_role_survives_a_round_trip_through_its_spelling() {
        for role in [OrgRole::Owner, OrgRole::Admin, OrgRole::Member] {
            assert_eq!(read_role(role.as_str()), Some(role));
        }
    }

    #[test]
    fn a_spelling_the_schema_does_not_hold_is_refused() {
        assert_eq!(read_role("root"), None);
        assert_eq!(read_role("Owner"), None);
        assert_eq!(read_role(""), None);
    }
}
