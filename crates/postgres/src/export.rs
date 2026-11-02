use ferristrace_application::repository;
use ferristrace_domain::audit::MAX_PAGE;
use ferristrace_domain::{
    ApiKeys, AuditFilter, AuditTrail, Clock, ExportSource, Logged, Orgs, PageSize, Projects,
    Recorded, Roll, Snapshot, Squads, StoreError, TenantId,
};

use crate::api_keys::PostgresApiKeys;
use crate::audit::PostgresAuditTrail;
use crate::decisions::read_recorded;
use crate::orgs::PostgresOrgs;
use crate::projects::PostgresProjects;
use crate::roster::PostgresRoster;
use crate::teams::PostgresTeams;
use crate::unit_of_work::{Postgres, SharedTx};

#[repository(domain = Export, backend = Postgres<C>, generics = <C: Clock + Clone + Send + Sync + 'static>)]
#[derive(Debug)]
pub struct PostgresExport<C> {
    tx: SharedTx,
    orgs: PostgresOrgs,
    projects: PostgresProjects,
    teams: PostgresTeams,
    roster: PostgresRoster,
    keys: PostgresApiKeys<C>,
    audit: PostgresAuditTrail<C>,
}

impl<C: Clock + Clone> PostgresExport<C> {
    pub fn new(backend: &Postgres<C>, tx: &SharedTx) -> Self {
        Self {
            tx: tx.clone(),
            orgs: PostgresOrgs::new(backend, tx),
            projects: PostgresProjects::new(backend, tx),
            teams: PostgresTeams::new(backend, tx),
            roster: PostgresRoster::new(backend, tx),
            keys: PostgresApiKeys::new(backend, tx),
            audit: PostgresAuditTrail::new(backend, tx),
        }
    }
}

impl<C: Clock + Clone + Send + Sync> ExportSource for PostgresExport<C> {
    async fn snapshot(&self, org: TenantId) -> Result<Snapshot, StoreError> {
        let card = self.orgs.only(org).await?.ok_or(StoreError::Unreachable)?;
        let projects = Projects::of(&self.projects, org).await?;
        let teams = Squads::of(&self.teams, org).await?;
        let members = Roll::of(&self.roster, org).await?;
        let keys = ApiKeys::list(&self.keys, org).await?;
        let decisions = self.decisions(org).await?;
        let audit = self.trail(org).await?;
        Ok(Snapshot {
            org: card,
            projects,
            teams,
            members,
            keys,
            decisions,
            audit,
        })
    }
}

impl<C: Clock + Clone + Send + Sync> PostgresExport<C> {
    async fn decisions(&self, org: TenantId) -> Result<Vec<Recorded>, StoreError> {
        let mut tx = self.tx.lock().await;
        let rows = sqlx::query(
            "SELECT d.project_id AS project_id, d.fingerprint AS fingerprint, \
             d.status AS status, d.owner_id AS owner_id, d.decided_at AS decided_at, \
             d.decided_by AS decided_by \
             FROM issue_decisions d JOIN projects p ON p.id = d.project_id \
             WHERE p.org_id = $1 ORDER BY d.project_id, d.fingerprint",
        )
        .bind(org.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the control plane refused to read the issue decisions");
            StoreError::Unreachable
        })?;
        rows.iter().map(read_recorded).collect()
    }

    async fn trail(&self, org: TenantId) -> Result<Vec<Logged>, StoreError> {
        let mut entries = Vec::new();
        let mut filter = AuditFilter::default();
        loop {
            let page = self
                .audit
                .list(org, &filter, PageSize::new(Some(MAX_PAGE)))
                .await?;
            entries.extend(page.entries);
            match page.next {
                Some(cursor) => filter.before = Some(cursor),
                None => return Ok(entries),
            }
        }
    }
}
