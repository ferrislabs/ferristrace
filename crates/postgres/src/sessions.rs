use ferristrace_domain::{
    AccountId, Agent, Clock, Email, LiveSession, SESSION_PREFIX, Session, SessionError, SessionId,
    SessionLifetime, SessionRoll, SessionStore, SessionToken, TenantId, UnixSeconds,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::accounts::read_portrait;

const TOKEN_BYTES: usize = 32;
const NANOS_PER_SECOND: i64 = 1_000_000_000;

#[derive(Debug, Clone)]
pub struct PostgresSessions<C> {
    pool: PgPool,
    clock: C,
    lifetime: SessionLifetime,
}

impl<C: Clock> PostgresSessions<C> {
    pub fn new(pool: PgPool, clock: C, lifetime: SessionLifetime) -> Self {
        Self {
            pool,
            clock,
            lifetime,
        }
    }

    pub async fn prune(&self) -> Result<u64, SessionError> {
        let now = seconds_to_nanos(self.clock.now());
        sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
            .bind(now)
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected())
            .map_err(|_| SessionError::StoreUnreachable)
    }
}

impl<C: Clock + Send + Sync> SessionStore for PostgresSessions<C> {
    async fn issue(
        &self,
        account: AccountId,
        org: Option<TenantId>,
        from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        let token = mint();
        let now = self.clock.now();
        let expires = self.lifetime.expiry_from(now, now);

        sqlx::query(
            "INSERT INTO sessions \
             (hash, id, account_id, org_id, created_at, last_seen_at, expires_at, user_agent) \
             VALUES ($1, $2, $3, $4, $5, $5, $6, $7)",
        )
        .bind(token.hash().as_bytes().as_slice())
        .bind(Uuid::new_v4())
        .bind(account.as_uuid())
        .bind(org.map(TenantId::as_uuid))
        .bind(seconds_to_nanos(now))
        .bind(seconds_to_nanos(expires))
        .bind(from.map(Agent::as_str))
        .execute(&self.pool)
        .await
        .map_err(|_| SessionError::StoreUnreachable)?;

        Ok(token)
    }

    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        let now = self.clock.now();
        let row = sqlx::query(
            "SELECT s.account_id AS account_id, a.email AS email, a.name AS name, \
                    a.photo_token AS photo_token, a.photo_type AS photo_type, \
                    s.org_id AS org_id, s.created_at AS created_at \
             FROM sessions s JOIN accounts a ON a.id = s.account_id \
             WHERE s.hash = $1 AND s.revoked_at IS NULL AND s.expires_at > $2",
        )
        .bind(token.hash().as_bytes().as_slice())
        .bind(seconds_to_nanos(now))
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| SessionError::StoreUnreachable)?
        .ok_or(SessionError::Unknown)?;

        let created = nanos_to_seconds(row.get("created_at"));
        let expires = self.lifetime.expiry_from(created, now);

        sqlx::query("UPDATE sessions SET last_seen_at = $2, expires_at = $3 WHERE hash = $1")
            .bind(token.hash().as_bytes().as_slice())
            .bind(seconds_to_nanos(now))
            .bind(seconds_to_nanos(expires))
            .execute(&self.pool)
            .await
            .map_err(|_| SessionError::StoreUnreachable)?;

        let email = Email::try_from(row.get::<String, _>("email").as_str())
            .map_err(|_| SessionError::StoreUnreachable)?;
        Ok(Session {
            account: AccountId::new(row.get("account_id")),
            email,
            name: row.get::<Option<String>, _>("name").map(Into::into),
            photo: read_portrait(&row),
            org: row
                .get::<Option<uuid::Uuid>, _>("org_id")
                .map(TenantId::new),
            expires_at: expires,
        })
    }

    async fn revoke(&self, token: &SessionToken) -> Result<(), SessionError> {
        sqlx::query("UPDATE sessions SET revoked_at = $2 WHERE hash = $1 AND revoked_at IS NULL")
            .bind(token.hash().as_bytes().as_slice())
            .bind(seconds_to_nanos(self.clock.now()))
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|_| SessionError::StoreUnreachable)
    }
}

impl<C: Clock + Send + Sync> SessionRoll for PostgresSessions<C> {
    async fn live(
        &self,
        account: AccountId,
        current: &SessionToken,
    ) -> Result<Vec<LiveSession>, SessionError> {
        let rows = sqlx::query(
            "SELECT id, created_at, last_seen_at, user_agent, (hash = $3) AS mine \
             FROM sessions \
             WHERE account_id = $1 AND revoked_at IS NULL AND expires_at > $2 \
             ORDER BY last_seen_at DESC",
        )
        .bind(account.as_uuid())
        .bind(seconds_to_nanos(self.clock.now()))
        .bind(current.hash().as_bytes().as_slice())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| SessionError::StoreUnreachable)?;

        Ok(rows
            .iter()
            .map(|row| LiveSession {
                id: SessionId::new(row.get("id")),
                from: row
                    .get::<Option<String>, _>("user_agent")
                    .as_deref()
                    .and_then(Agent::new),
                created_at: nanos_to_seconds(row.get("created_at")),
                last_seen_at: nanos_to_seconds(row.get("last_seen_at")),
                current: row.get("mine"),
            })
            .collect())
    }

    async fn revoke_one(&self, account: AccountId, id: SessionId) -> Result<bool, SessionError> {
        sqlx::query(
            "UPDATE sessions SET revoked_at = $3 \
             WHERE account_id = $1 AND id = $2 AND revoked_at IS NULL",
        )
        .bind(account.as_uuid())
        .bind(id.as_uuid())
        .bind(seconds_to_nanos(self.clock.now()))
        .execute(&self.pool)
        .await
        .map(|done| done.rows_affected() > 0)
        .map_err(|_| SessionError::StoreUnreachable)
    }

    async fn revoke_others(
        &self,
        account: AccountId,
        current: &SessionToken,
    ) -> Result<u64, SessionError> {
        sqlx::query(
            "UPDATE sessions SET revoked_at = $3 \
             WHERE account_id = $1 AND hash <> $2 AND revoked_at IS NULL",
        )
        .bind(account.as_uuid())
        .bind(current.hash().as_bytes().as_slice())
        .bind(seconds_to_nanos(self.clock.now()))
        .execute(&self.pool)
        .await
        .map(|done| done.rows_affected())
        .map_err(|_| SessionError::StoreUnreachable)
    }
}

fn mint() -> SessionToken {
    let bytes: [u8; TOKEN_BYTES] = rand::random();
    SessionToken::from_plaintext(&format!("{SESSION_PREFIX}{}", hex::encode(bytes)))
}

fn seconds_to_nanos(instant: UnixSeconds) -> i64 {
    i64::try_from(instant.get())
        .unwrap_or(i64::MAX / NANOS_PER_SECOND)
        .saturating_mul(NANOS_PER_SECOND)
}

fn nanos_to_seconds(nanos: i64) -> UnixSeconds {
    UnixSeconds::new(u64::try_from(nanos / NANOS_PER_SECOND).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::SESSION_PREFIX;

    use super::mint;

    #[test]
    fn a_minted_token_carries_the_prefix_and_enough_randomness() {
        let token = mint();

        assert!(token.looks_like_one());
        assert_eq!(token.exposed().len(), SESSION_PREFIX.len() + 64);
    }

    #[test]
    fn two_minted_tokens_differ() {
        assert_ne!(mint().exposed(), mint().exposed());
    }
}
