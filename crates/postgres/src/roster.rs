use ferristrace_application::repository;
use ferristrace_domain::organisation::{Holder, Joined};
use ferristrace_domain::{AccountId, Email, Member, OrgRole, Roll, StoreError, TenantId};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

use crate::accounts::read_portrait;
use crate::memberships::read_role;
use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = Roster, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresRoster {
    tx: SharedTx,
}

impl PostgresRoster {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl Roll for PostgresRoster {
    async fn of(&self, org: TenantId) -> Result<Vec<Member>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT m.account_id AS account_id, a.email AS email, a.name AS name, \
             a.photo_token AS photo_token, a.photo_type AS photo_type, \
             m.role::text AS role, m.created_at AS created_at \
             FROM org_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.org_id = $1 ORDER BY m.role, a.email",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to read the members");
            StoreError::Unreachable
        })?;

        rows.iter().map(read_member).collect()
    }

    async fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> Result<Option<Member>, StoreError> {
        let mut tx = self.tx.lock().await;
        let found = sqlx::query(
            "UPDATE org_members m SET role = $3::org_role FROM accounts a \
             WHERE m.org_id = $1 AND m.account_id = $2 AND a.id = m.account_id \
             RETURNING m.account_id AS account_id, a.email AS email, a.name AS name, \
             a.photo_token AS photo_token, a.photo_type AS photo_type, \
             m.role::text AS role, m.created_at AS created_at",
        )
        .bind(org.as_uuid())
        .bind(account.as_uuid())
        .bind(role.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to change a role");
            StoreError::Unreachable
        })?;

        found.as_ref().map(read_member).transpose()
    }

    async fn remove(&self, org: TenantId, account: AccountId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("DELETE FROM org_members WHERE org_id = $1 AND account_id = $2")
            .bind(org.as_uuid())
            .bind(account.as_uuid())
            .execute(&mut **tx)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to remove a member");
                StoreError::Unreachable
            })
    }

    async fn standing_of(&self, email: &Email) -> Result<Option<Holder>, StoreError> {
        let mut tx = self.tx.lock().await;
        let standing = sqlx::query(
            "SELECT id AS id, name AS name, photo_token AS photo_token, \
             photo_type AS photo_type FROM accounts WHERE email = $1",
        )
        .bind(email.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| unreachable(&error))?;

        Ok(standing.as_ref().map(|row| Holder {
            account: AccountId::new(row.get("id")),
            name: row.get::<Option<String>, _>("name").map(Into::into),
            photo: read_portrait(row),
        }))
    }

    async fn create_locked(&self, email: &Email) -> Result<AccountId, StoreError> {
        let account = Uuid::new_v4();
        let mut tx = self.tx.lock().await;
        sqlx::query("INSERT INTO accounts (id, email) VALUES ($1, $2)")
            .bind(account)
            .bind(email.as_str())
            .execute(&mut **tx)
            .await
            .map_err(|error| unreachable(&error))?;
        Ok(AccountId::new(account))
    }

    async fn join(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> Result<Option<Joined>, StoreError> {
        let mut tx = self.tx.lock().await;
        let joined = sqlx::query(
            "INSERT INTO org_members (org_id, account_id, role) VALUES ($1, $2, $3::org_role) \
             ON CONFLICT (org_id, account_id) DO NOTHING \
             RETURNING role::text AS role, created_at AS created_at",
        )
        .bind(org.as_uuid())
        .bind(account.as_uuid())
        .bind(role.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| unreachable(&error))?;

        joined
            .as_ref()
            .map(|row| {
                Ok(Joined {
                    role: known_role(row)?,
                    joined_at_unix_nanos: read_nanos(row),
                })
            })
            .transpose()
    }
}

fn read_member(row: &PgRow) -> Result<Member, StoreError> {
    Ok(Member {
        account: AccountId::new(row.get("account_id")),
        email: Email::try_from(row.get::<String, _>("email").as_str()).map_err(|error| {
            tracing::error!(%error, "the control plane holds an address nothing can send to");
            StoreError::Unreachable
        })?,
        name: row.get::<Option<String>, _>("name").map(Into::into),
        photo: read_portrait(row),
        role: known_role(row)?,
        joined_at_unix_nanos: read_nanos(row),
    })
}

fn known_role(row: &PgRow) -> Result<OrgRole, StoreError> {
    let spelling: String = row.get("role");
    read_role(&spelling).ok_or_else(|| {
        tracing::error!(role = %spelling, "the control plane holds an unknown role");
        StoreError::Unreachable
    })
}

fn read_nanos(row: &PgRow) -> u64 {
    u64::try_from(row.get::<i64, _>("created_at")).unwrap_or_default()
}

fn unreachable(error: &sqlx::Error) -> StoreError {
    tracing::error!(%error, "the control plane refused an admission");
    StoreError::Unreachable
}
