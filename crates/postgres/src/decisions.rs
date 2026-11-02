use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, Decision, Fingerprint, IssueDecisions, ProjectId, ReadError, Recorded, Status,
};
use sqlx::Row;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

const READ: &str = "SELECT project_id, fingerprint, status, owner_id, decided_at, decided_by \
     FROM issue_decisions \
     WHERE (project_id, fingerprint) IN (SELECT * FROM unnest($1::uuid[], $2::bytea[]))";

const DECIDE: &str = "INSERT INTO issue_decisions \
       (project_id, fingerprint, status, decided_at, decided_by) \
     VALUES ($1, $2, $3, $4, $5) \
     ON CONFLICT (project_id, fingerprint) \
     DO UPDATE SET status = $3, decided_at = $4, decided_by = $5";

const ASSIGN: &str = "INSERT INTO issue_decisions \
       (project_id, fingerprint, status, owner_id, decided_at, decided_by) \
     VALUES ($1, $2, $3, $4, $5, $6) \
     ON CONFLICT (project_id, fingerprint) \
     DO UPDATE SET owner_id = $4";

#[repository(domain = Decisions, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresDecisions {
    tx: SharedTx,
}

impl PostgresDecisions {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl IssueDecisions for PostgresDecisions {
    async fn read_many(&self, of: &[(ProjectId, Fingerprint)]) -> Result<Vec<Recorded>, ReadError> {
        let mut tx = self.tx.lock().await;
        if of.is_empty() {
            return Ok(Vec::new());
        }
        let projects: Vec<Uuid> = of.iter().map(|(project, _)| project.as_uuid()).collect();
        let prints: Vec<Vec<u8>> = of
            .iter()
            .map(|(_, print)| print.as_bytes().to_vec())
            .collect();

        let rows = sqlx::query(READ)
            .bind(&projects)
            .bind(&prints)
            .fetch_all(&mut **tx)
            .await
            .map_err(|_| ReadError::StoreUnreachable)?;

        rows.iter()
            .map(|row| {
                let print: Vec<u8> = row.get("fingerprint");
                let bytes = print
                    .try_into()
                    .map_err(|_| ReadError::StoreUnreachable)
                    .map(Fingerprint::new)?;
                Ok(Recorded {
                    project: ProjectId::new(row.get("project_id")),
                    print: bytes,
                    decision: Decision {
                        status: Status::parse(row.get::<String, _>("status").as_str())
                            .unwrap_or_default(),
                        owner: row.get::<Option<Uuid>, _>("owner_id").map(AccountId::new),
                        decided_at_unix_nanos: unsigned(row.get("decided_at")),
                        decided_by: row.get::<Option<Uuid>, _>("decided_by").map(AccountId::new),
                    },
                })
            })
            .collect()
    }

    async fn decide(
        &self,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> Result<(), ReadError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(DECIDE)
            .bind(project.as_uuid())
            .bind(print.as_bytes().as_slice())
            .bind(status.as_str())
            .bind(signed(at_unix_nanos))
            .bind(by.as_uuid())
            .execute(&mut **tx)
            .await
            .map(|_| ())
            .map_err(|_| ReadError::StoreUnreachable)
    }

    async fn assign(
        &self,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
        by: AccountId,
        at_unix_nanos: u64,
    ) -> Result<(), ReadError> {
        let mut tx = self.tx.lock().await;
        sqlx::query(ASSIGN)
            .bind(project.as_uuid())
            .bind(print.as_bytes().as_slice())
            .bind(Status::Unresolved.as_str())
            .bind(owner.map(AccountId::as_uuid))
            .bind(signed(at_unix_nanos))
            .bind(by.as_uuid())
            .execute(&mut **tx)
            .await
            .map(|_| ())
            .map_err(|_| ReadError::StoreUnreachable)
    }
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{ASSIGN, DECIDE, READ};

    #[test]
    fn every_statement_is_keyed_by_the_pair_that_defines_an_issue() {
        assert!(READ.contains("(project_id, fingerprint) IN"));
        for statement in [DECIDE, ASSIGN] {
            assert!(statement.contains("ON CONFLICT (project_id, fingerprint)"));
        }
    }

    #[test]
    fn assigning_an_owner_never_moves_the_moment_a_status_was_decided() {
        assert!(
            !ASSIGN.contains("DO UPDATE SET owner_id = $4, decided_at"),
            "a reassignment that moved decided_at would clear a regression"
        );
        assert_eq!(ASSIGN.matches("DO UPDATE SET owner_id = $4").count(), 1);
        assert!(DECIDE.contains("DO UPDATE SET status = $3, decided_at = $4"));
    }

    #[test]
    fn deciding_a_status_never_clears_an_owner() {
        assert!(
            !DECIDE.contains("owner_id"),
            "resolving an issue must leave whoever owns it in place"
        );
    }
}
