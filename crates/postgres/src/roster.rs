use std::time::Duration;

use ferristrace_domain::{
    AccountId, Admitted, Clock, Email, Member, OrgRole, ReadError, Roll, SecretHash, TenantId,
    UnixSeconds,
};
use sqlx::{PgPool, Row, postgres::PgRow};
use tokio::task;
use uuid::Uuid;

use crate::accounts::read_portrait;
use crate::memberships::read_role;
use ferristrace_infra::Argon2Hasher;

const NANOS_PER_SECOND: i64 = 1_000_000_000;

const WAY_IN_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone)]
pub struct PostgresRoster<C> {
    pool: PgPool,
    hasher: Argon2Hasher,
    clock: C,
}

impl<C: Clock> PostgresRoster<C> {
    pub fn new(pool: PgPool, hasher: Argon2Hasher, clock: C) -> Self {
        Self {
            pool,
            hasher,
            clock,
        }
    }

    async fn locked(&self) -> Result<String, ReadError> {
        let hasher = self.hasher.clone();
        task::spawn_blocking(move || hasher.locked())
            .await
            .map_err(|_| ReadError::StoreUnreachable)?
            .map(|hash| hash.encoded().to_owned())
            .map_err(|error| {
                tracing::error!(%error, "a new account could not be locked");
                ReadError::StoreUnreachable
            })
    }
}

impl<C: Clock + Send + Sync> Roll for PostgresRoster<C> {
    async fn of(&self, org: TenantId) -> Result<Vec<Member>, ReadError> {
        let rows = sqlx::query(
            "SELECT m.account_id AS account_id, a.email AS email, a.name AS name, \
             a.photo_token AS photo_token, a.photo_type AS photo_type, \
             m.role::text AS role, m.created_at AS created_at \
             FROM org_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.org_id = $1 ORDER BY m.role, a.email",
        )
        .bind(org.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to read the members");
            ReadError::StoreUnreachable
        })?;

        rows.iter().map(read_member).collect()
    }

    async fn grant(
        &self,
        org: TenantId,
        account: AccountId,
        role: OrgRole,
    ) -> Result<Option<Member>, ReadError> {
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
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to change a role");
            ReadError::StoreUnreachable
        })?;

        found.as_ref().map(read_member).transpose()
    }

    async fn remove(&self, org: TenantId, account: AccountId) -> Result<bool, ReadError> {
        sqlx::query("DELETE FROM org_members WHERE org_id = $1 AND account_id = $2")
            .bind(org.as_uuid())
            .bind(account.as_uuid())
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|error| {
                tracing::error!(%error, "the control plane refused to remove a member");
                ReadError::StoreUnreachable
            })
    }

    async fn admit(
        &self,
        org: TenantId,
        email: &Email,
        role: OrgRole,
        way_in: SecretHash,
    ) -> Result<Admitted, ReadError> {
        let locked = self.locked().await?;
        let now = to_nanos(self.clock.now());
        let expires = to_nanos(
            self.clock
                .now()
                .saturating_add_secs(WAY_IN_WINDOW.as_secs()),
        );

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| unreachable(&error))?;

        let standing = sqlx::query(
            "SELECT id AS id, name AS name, photo_token AS photo_token, \
             photo_type AS photo_type FROM accounts WHERE email = $1",
        )
        .bind(email.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| unreachable(&error))?;

        let (account, name, photo) = match &standing {
            Some(row) => (
                row.get::<Uuid, _>("id"),
                row.get::<Option<String>, _>("name"),
                read_portrait(row),
            ),
            None => (
                locked_account(&mut *transaction, email, &locked)
                    .await
                    .map_err(|error| unreachable(&error))?,
                None,
                None,
            ),
        };

        let joined = sqlx::query(
            "INSERT INTO org_members (org_id, account_id, role) VALUES ($1, $2, $3::org_role) \
             ON CONFLICT (org_id, account_id) DO NOTHING \
             RETURNING account_id AS account_id, role::text AS role, created_at AS created_at",
        )
        .bind(org.as_uuid())
        .bind(account)
        .bind(role.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| unreachable(&error))?;

        let Some(row) = joined else {
            transaction
                .rollback()
                .await
                .map_err(|error| unreachable(&error))?;
            return Ok(Admitted::AlreadyIn);
        };

        let member = Member {
            account: AccountId::new(row.get("account_id")),
            email: email.clone(),
            name: name.map(Into::into),
            photo,
            role: known_role(&row)?,
            joined_at_unix_nanos: read_nanos(&row),
        };

        if standing.is_some() {
            transaction
                .commit()
                .await
                .map_err(|error| unreachable(&error))?;
            return Ok(Admitted::Standing(member));
        }

        sqlx::query(
            "INSERT INTO password_resets (digest, account_id, created_at, expires_at) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(way_in.as_bytes().as_slice())
        .bind(account)
        .bind(now)
        .bind(expires)
        .execute(&mut *transaction)
        .await
        .map_err(|error| unreachable(&error))?;

        transaction
            .commit()
            .await
            .map_err(|error| unreachable(&error))?;
        Ok(Admitted::Awaiting(member))
    }
}

async fn locked_account<'c, E>(
    executor: E,
    email: &Email,
    locked: &str,
) -> Result<Uuid, sqlx::Error>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    let account = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(account)
        .bind(email.as_str())
        .bind(locked)
        .execute(executor)
        .await
        .map(|_| account)
}

fn read_member(row: &PgRow) -> Result<Member, ReadError> {
    Ok(Member {
        account: AccountId::new(row.get("account_id")),
        email: Email::try_from(row.get::<String, _>("email").as_str()).map_err(|error| {
            tracing::error!(%error, "the control plane holds an address nothing can send to");
            ReadError::StoreUnreachable
        })?,
        name: row.get::<Option<String>, _>("name").map(Into::into),
        photo: read_portrait(row),
        role: known_role(row)?,
        joined_at_unix_nanos: read_nanos(row),
    })
}

fn known_role(row: &PgRow) -> Result<OrgRole, ReadError> {
    let spelling: String = row.get("role");
    read_role(&spelling).ok_or_else(|| {
        tracing::error!(role = %spelling, "the control plane holds an unknown role");
        ReadError::StoreUnreachable
    })
}

fn read_nanos(row: &PgRow) -> u64 {
    u64::try_from(row.get::<i64, _>("created_at")).unwrap_or_default()
}

fn to_nanos(instant: UnixSeconds) -> i64 {
    i64::try_from(instant.get())
        .unwrap_or(i64::MAX / NANOS_PER_SECOND)
        .saturating_mul(NANOS_PER_SECOND)
}

fn unreachable(error: &sqlx::Error) -> ReadError {
    tracing::error!(%error, "the control plane refused an admission");
    ReadError::StoreUnreachable
}
