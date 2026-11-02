use std::collections::BTreeMap;

use ferristrace_application::repository;
use ferristrace_domain::{
    AccountId, Named, Owned, ProjectId, Squads, StoreError, Team, TeamId, TeamName, TenantId,
};
use sqlx::Row;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = Teams, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresTeams {
    tx: SharedTx,
}

impl PostgresTeams {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }
}

impl Squads for PostgresTeams {
    async fn of(&self, org: TenantId) -> Result<Vec<Team>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT id AS id, name AS name, created_at AS created_at \
             FROM teams WHERE org_id = $1 ORDER BY name, id",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "read the teams"))?;

        let seated = sqlx::query(
            "SELECT team_id AS team_id, account_id AS account_id \
             FROM team_members WHERE org_id = $1 ORDER BY account_id",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "read who is on the teams"))?;

        let owned = sqlx::query(
            "SELECT team_id AS team_id, id AS id, name AS name \
             FROM projects WHERE org_id = $1 AND team_id IS NOT NULL ORDER BY name, id",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "read what the teams own"))?;

        let mut members: BTreeMap<Uuid, Vec<AccountId>> = BTreeMap::new();
        for row in &seated {
            members
                .entry(row.get("team_id"))
                .or_default()
                .push(AccountId::new(row.get("account_id")));
        }

        let mut projects: BTreeMap<Uuid, Vec<Owned>> = BTreeMap::new();
        for row in &owned {
            projects.entry(row.get("team_id")).or_default().push(Owned {
                id: ProjectId::new(row.get("id")),
                name: row.get::<String, _>("name").into(),
            });
        }

        Ok(rows
            .into_iter()
            .map(|row| {
                let id: Uuid = row.get("id");
                Team {
                    id: TeamId::new(id),
                    name: row.get::<String, _>("name").into(),
                    created_at_unix_nanos: u64::try_from(row.get::<i64, _>("created_at"))
                        .unwrap_or_default(),
                    members: members.remove(&id).unwrap_or_default(),
                    projects: projects.remove(&id).unwrap_or_default(),
                }
            })
            .collect())
    }

    async fn create(&self, org: TenantId, name: &TeamName) -> Result<Named, StoreError> {
        let mut tx = self.tx.lock().await;
        let written = sqlx::query(
            "INSERT INTO teams (id, org_id, name) VALUES ($1, $2, $3) \
             ON CONFLICT (org_id, name) DO NOTHING RETURNING id AS id",
        )
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(name.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "create a team"))?;

        Ok(named(written.is_some()))
    }

    async fn rename(
        &self,
        org: TenantId,
        team: TeamId,
        name: &TeamName,
    ) -> Result<Named, StoreError> {
        let mut tx = self.tx.lock().await;
        let clash =
            sqlx::query("SELECT 1 AS one FROM teams WHERE org_id = $1 AND name = $2 AND id <> $3")
                .bind(org.as_uuid())
                .bind(name.as_str())
                .bind(team.as_uuid())
                .fetch_optional(&mut **tx)
                .await
                .map_err(|error| unreachable(&error, "look a team name up"))?;
        if clash.is_some() {
            return Ok(Named::Taken);
        }

        match sqlx::query("UPDATE teams SET name = $3 WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(team.as_uuid())
            .bind(name.as_str())
            .execute(&mut **tx)
            .await
        {
            Ok(_) => Ok(Named::Free),
            Err(sqlx::Error::Database(refusal)) if refusal.is_unique_violation() => {
                Ok(Named::Taken)
            }
            Err(error) => Err(unreachable(&error, "rename a team")),
        }
    }

    async fn disband(&self, org: TenantId, team: TeamId) -> Result<bool, StoreError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("DELETE FROM teams WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(team.as_uuid())
            .execute(&mut **tx)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|error| unreachable(&error, "disband a team"))
    }

    async fn seat(
        &self,
        org: TenantId,
        team: TeamId,
        members: &[AccountId],
    ) -> Result<bool, StoreError> {
        let accounts: Vec<Uuid> = members.iter().map(|member| member.as_uuid()).collect();

        let mut tx = self.tx.lock().await;

        if !holds(&mut **tx, org, team).await? {
            return Ok(false);
        }

        sqlx::query("DELETE FROM team_members WHERE org_id = $1 AND team_id = $2")
            .bind(org.as_uuid())
            .bind(team.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(|error| unreachable(&error, "clear a team"))?;

        sqlx::query(
            "INSERT INTO team_members (org_id, team_id, account_id) \
             SELECT m.org_id, $2, m.account_id FROM org_members m \
             WHERE m.org_id = $1 AND m.account_id = ANY($3)",
        )
        .bind(org.as_uuid())
        .bind(team.as_uuid())
        .bind(&accounts)
        .execute(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "seat a team"))?;

        Ok(true)
    }

    async fn own(
        &self,
        org: TenantId,
        team: TeamId,
        projects: &[ProjectId],
    ) -> Result<bool, StoreError> {
        let claimed: Vec<Uuid> = projects.iter().map(|project| project.as_uuid()).collect();

        let mut tx = self.tx.lock().await;

        if !holds(&mut **tx, org, team).await? {
            return Ok(false);
        }

        sqlx::query(
            "UPDATE projects SET team_id = NULL \
             WHERE org_id = $1 AND team_id = $2 AND NOT (id = ANY($3))",
        )
        .bind(org.as_uuid())
        .bind(team.as_uuid())
        .bind(&claimed)
        .execute(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "let a project go"))?;

        sqlx::query("UPDATE projects SET team_id = $2 WHERE org_id = $1 AND id = ANY($3)")
            .bind(org.as_uuid())
            .bind(team.as_uuid())
            .bind(&claimed)
            .execute(&mut **tx)
            .await
            .map_err(|error| unreachable(&error, "hand a team its projects"))?;

        Ok(true)
    }
}

async fn holds<'c, E>(executor: E, org: TenantId, team: TeamId) -> Result<bool, StoreError>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    sqlx::query("SELECT 1 AS one FROM teams WHERE org_id = $1 AND id = $2")
        .bind(org.as_uuid())
        .bind(team.as_uuid())
        .fetch_optional(executor)
        .await
        .map(|found| found.is_some())
        .map_err(|error| unreachable(&error, "look a team up"))
}

fn named(written: bool) -> Named {
    if written { Named::Free } else { Named::Taken }
}

fn unreachable(error: &sqlx::Error, what: &str) -> StoreError {
    tracing::error!(%error, "the control plane refused to {what}");
    StoreError::Unreachable
}
