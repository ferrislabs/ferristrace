use ferristrace_application::repository;
use ferristrace_domain::{
    Assigned, Created, OwnerTeam, Project, ProjectId, ProjectName, Projects, ReadError, Renamed,
    TeamId, TenantId,
};
use sqlx::Row;
use uuid::Uuid;

use crate::unit_of_work::{Postgres, SharedTx};

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("the control plane refused the project query")]
    ControlPlane(#[from] sqlx::Error),
    #[error("no organisation carries the id {0}")]
    NoSuchOrg(Uuid),
    #[error("that organisation already holds a project called {0}")]
    Taken(String),
}

#[repository(domain = Projects, backend = Postgres<C>, generics = <C: Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresProjects {
    tx: SharedTx,
}

impl PostgresProjects {
    pub fn new<C>(_backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self { tx: tx.clone() }
    }

    pub async fn claim(
        &self,
        org: TenantId,
        name: &ProjectName,
    ) -> Result<ProjectId, ProjectError> {
        let mut tx = self.tx.lock().await;
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO projects (id, org_id, name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(org.as_uuid())
            .bind(name.as_str())
            .execute(&mut **tx)
            .await
            .map_err(|error| match &error {
                sqlx::Error::Database(refusal) if refusal.is_unique_violation() => {
                    ProjectError::Taken(name.as_str().to_owned())
                }
                sqlx::Error::Database(refusal) if refusal.is_foreign_key_violation() => {
                    ProjectError::NoSuchOrg(org.as_uuid())
                }
                _ => ProjectError::ControlPlane(error),
            })?;
        Ok(ProjectId::new(id))
    }

    pub async fn only(&self, org: TenantId) -> Result<Option<ProjectId>, ProjectError> {
        let mut tx = self.tx.lock().await;
        let found = sqlx::query(
            "SELECT id AS id FROM projects WHERE org_id = $1 ORDER BY created_at LIMIT 2",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await?;
        if found.len() == 1 {
            Ok(Some(ProjectId::new(found[0].get("id"))))
        } else {
            Ok(None)
        }
    }
}

impl Projects for PostgresProjects {
    async fn of(&self, tenant: TenantId) -> Result<Vec<Project>, ReadError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT p.id AS id, p.name AS name, p.created_at AS created_at, \
             t.id AS team_id, t.name AS team_name, \
             (SELECT count(*) FROM api_keys k \
              WHERE k.project_id = p.id AND k.revoked_at IS NULL) AS keys \
             FROM projects p LEFT JOIN teams t ON t.id = p.team_id \
             WHERE p.org_id = $1 ORDER BY p.name, p.id",
        )
        .bind(tenant.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "read the projects"))?;

        Ok(rows
            .into_iter()
            .map(|row| Project {
                id: ProjectId::new(row.get("id")),
                org: tenant,
                name: row.get::<String, _>("name").into(),
                created_at_unix_nanos: u64::try_from(row.get::<i64, _>("created_at"))
                    .unwrap_or_default(),
                team: row.get::<Option<Uuid>, _>("team_id").map(|id| OwnerTeam {
                    id: TeamId::new(id),
                    name: row.get::<String, _>("team_name").into(),
                }),
                keys: u32::try_from(row.get::<i64, _>("keys")).unwrap_or(u32::MAX),
            })
            .collect())
    }

    async fn create(&self, org: TenantId, name: &ProjectName) -> Result<Created, ReadError> {
        let mut tx = self.tx.lock().await;
        let written = sqlx::query(
            "INSERT INTO projects (id, org_id, name) VALUES ($1, $2, $3) \
             ON CONFLICT (org_id, name) DO NOTHING RETURNING id AS id",
        )
        .bind(Uuid::new_v4())
        .bind(org.as_uuid())
        .bind(name.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "create a project"))?;

        Ok(if written.is_some() {
            Created::Done
        } else {
            Created::Taken
        })
    }

    async fn rename(
        &self,
        org: TenantId,
        project: ProjectId,
        name: &ProjectName,
    ) -> Result<Renamed, ReadError> {
        let mut tx = self.tx.lock().await;
        match sqlx::query("UPDATE projects SET name = $3 WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(project.as_uuid())
            .bind(name.as_str())
            .execute(&mut **tx)
            .await
        {
            Ok(done) if done.rows_affected() > 0 => Ok(Renamed::Done),
            Ok(_) => Ok(Renamed::Gone),
            Err(sqlx::Error::Database(refusal)) if refusal.is_unique_violation() => {
                Ok(Renamed::Taken)
            }
            Err(error) => Err(unreachable(&error, "rename a project")),
        }
    }

    async fn forget(&self, org: TenantId, project: ProjectId) -> Result<bool, ReadError> {
        let mut tx = self.tx.lock().await;
        sqlx::query("DELETE FROM projects WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(project.as_uuid())
            .execute(&mut **tx)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|error| unreachable(&error, "forget a project"))
    }

    async fn assign(
        &self,
        org: TenantId,
        project: ProjectId,
        team: Option<TeamId>,
    ) -> Result<Assigned, ReadError> {
        let mut tx = self.tx.lock().await;
        let wanted = team.map(TeamId::as_uuid);
        let done = sqlx::query(
            "UPDATE projects SET team_id = $3 WHERE org_id = $1 AND id = $2 \
             AND ($3::uuid IS NULL OR EXISTS \
                  (SELECT 1 FROM teams WHERE org_id = $1 AND id = $3))",
        )
        .bind(org.as_uuid())
        .bind(project.as_uuid())
        .bind(wanted)
        .execute(&mut **tx)
        .await
        .map_err(|error| unreachable(&error, "hand a project to a team"))?;

        if done.rows_affected() > 0 {
            return Ok(Assigned::Done);
        }

        let there = sqlx::query("SELECT 1 AS one FROM projects WHERE org_id = $1 AND id = $2")
            .bind(org.as_uuid())
            .bind(project.as_uuid())
            .fetch_optional(&mut **tx)
            .await
            .map_err(|error| unreachable(&error, "look a project up"))?;

        Ok(if there.is_some() {
            Assigned::NoSuchTeam
        } else {
            Assigned::Gone
        })
    }
}

fn unreachable(error: &sqlx::Error, what: &str) -> ReadError {
    tracing::error!(%error, "the control plane refused to {what}");
    ReadError::StoreUnreachable
}
