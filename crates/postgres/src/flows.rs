use std::time::Duration;

use ferristrace_application::repository;
use ferristrace_domain::{Clock, CodeVerifier, FlowError, OidcFlows, PendingFlow, UnixSeconds};
use sqlx::Row;

use crate::unit_of_work::{Postgres, SharedTx};

const NANOS_PER_SECOND: i64 = 1_000_000_000;
pub(crate) const DEFAULT_WINDOW: Duration = Duration::from_secs(600);

#[repository(domain = Flows, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresFlows<C> {
    tx: SharedTx,
    clock: C,
    window: Duration,
}

impl<C: Clock> PostgresFlows<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self
    where
        C: Clone,
    {
        Self {
            tx: tx.clone(),
            clock: backend.clock().clone(),
            window: backend.flow_window,
        }
    }

    pub async fn prune(&self) -> Result<u64, FlowError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("DELETE FROM oidc_flows WHERE expires_at <= $1")
            .bind(to_nanos(self.clock.now()))
            .execute(&mut **tx)
            .await
            .map(|done| done.rows_affected())
            .map_err(|_| FlowError::StoreUnreachable)
    }
}

impl<C: Clock + Send + Sync> OidcFlows for PostgresFlows<C> {
    async fn begin(&self, flow: &PendingFlow) -> Result<(), FlowError> {
        let mut tx = self.tx.lock().await;
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
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(|_| FlowError::StoreUnreachable)
    }

    async fn take(&self, state: &str) -> Result<PendingFlow, FlowError> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(
            "DELETE FROM oidc_flows WHERE state = $1 AND expires_at > $2 \
             RETURNING nonce AS nonce, code_verifier AS code_verifier, \
             redirect_uri AS redirect_uri",
        )
        .bind(state)
        .bind(to_nanos(self.clock.now()))
        .fetch_optional(&mut **tx)
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
