use std::time::Duration;

use ferristrace_domain::{Clock, CodeVerifier, FlowError, OidcFlows, PendingFlow, UnixSeconds};
use sqlx::{PgPool, Row};

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const DEFAULT_WINDOW: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct PostgresFlows<C> {
    pool: PgPool,
    clock: C,
    window: Duration,
}

impl<C: Clock> PostgresFlows<C> {
    pub fn new(pool: PgPool, clock: C) -> Self {
        Self {
            pool,
            clock,
            window: DEFAULT_WINDOW,
        }
    }

    #[must_use]
    pub fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    pub async fn prune(&self) -> Result<u64, FlowError> {
        sqlx::query("DELETE FROM oidc_flows WHERE expires_at <= $1")
            .bind(to_nanos(self.clock.now()))
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected())
            .map_err(|_| FlowError::StoreUnreachable)
    }
}

impl<C: Clock + Send + Sync> OidcFlows for PostgresFlows<C> {
    async fn begin(&self, flow: &PendingFlow) -> Result<(), FlowError> {
        let now = self.clock.now();
        let expires = now.saturating_add_secs(self.window.as_secs());

        sqlx::query(
            "INSERT INTO oidc_flows \
             (state, nonce, code_verifier, redirect_uri, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&flow.state)
        .bind(&flow.nonce)
        .bind(flow.verifier.exposed())
        .bind(&flow.redirect_uri)
        .bind(to_nanos(now))
        .bind(to_nanos(expires))
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|_| FlowError::StoreUnreachable)
    }

    async fn take(&self, state: &str) -> Result<PendingFlow, FlowError> {
        let row = sqlx::query(
            "DELETE FROM oidc_flows WHERE state = $1 AND expires_at > $2 \
             RETURNING nonce AS nonce, code_verifier AS code_verifier, \
             redirect_uri AS redirect_uri",
        )
        .bind(state)
        .bind(to_nanos(self.clock.now()))
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| FlowError::StoreUnreachable)?
        .ok_or(FlowError::Unknown)?;

        Ok(PendingFlow {
            state: state.to_owned(),
            nonce: row.get("nonce"),
            verifier: CodeVerifier::from_plaintext(row.get("code_verifier")),
            redirect_uri: row.get("redirect_uri"),
        })
    }
}

fn to_nanos(instant: UnixSeconds) -> i64 {
    i64::try_from(instant.get())
        .unwrap_or(i64::MAX / NANOS_PER_SECOND)
        .saturating_mul(NANOS_PER_SECOND)
}
