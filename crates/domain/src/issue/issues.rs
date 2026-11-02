use super::entities::Issue;
use super::errors::IssueError;
use super::ports::{IssueDecisions, IssueReader};
use super::value_objects::{Fingerprint, IssueFilter, MAX_LIMIT, Occurrence, Status};
use crate::access::{ApiKeys, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::clock::Clock;
use crate::explore::Window;
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, OrgRole};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug)]
pub struct Issues<S, G, K, R, P, C> {
    readers: Readers<S, G, K>,
    reader: R,
    decisions: P,
    clock: C,
}

impl<S, G, K, R, P, C> Issues<S, G, K, R, P, C>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    R: IssueReader,
    P: IssueDecisions,
    C: Clock,
{
    pub fn new(readers: Readers<S, G, K>, reader: R, decisions: P, clock: C) -> Self {
        Self {
            readers,
            reader,
            decisions,
            clock,
        }
    }

    fn now_unix_nanos(&self) -> u64 {
        self.clock.now().get().saturating_mul(NANOS_PER_SECOND)
    }

    async fn acting(&self, token: &SessionToken, org: TenantId) -> Result<AccountId, IssueError> {
        let who = self.readers.whoami().of(token).await?;
        if who.may_act_in(org, OrgRole::Member) {
            Ok(who.account)
        } else {
            Err(IssueError::NotYours)
        }
    }

    pub async fn list(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &IssueFilter,
    ) -> Result<Vec<Issue>, IssueError> {
        let project = self.readers.grant(who, org).await?.narrow(filter.project)?;
        let counted = if project == filter.project {
            self.reader.issues(org, filter).await?
        } else {
            let mut narrowed = filter.clone();
            narrowed.project = project;
            self.reader.issues(org, &narrowed).await?
        };
        let keys: Vec<(ProjectId, Fingerprint)> =
            counted.iter().map(|one| (one.project, one.print)).collect();
        let recorded = self.decisions.read_many(&keys).await?;
        Ok(counted
            .into_iter()
            .map(|one| {
                let decision = recorded
                    .iter()
                    .find(|held| held.project == one.project && held.print == one.print)
                    .map(|held| held.decision)
                    .unwrap_or_default();
                Issue::of(one, decision)
            })
            .collect())
    }

    pub async fn one(
        &self,
        who: &Reader,
        org: TenantId,
        project: Option<ProjectId>,
        print: Fingerprint,
        window: Window,
    ) -> Result<Issue, IssueError> {
        let mut filter = IssueFilter::over(window).at_most(1);
        filter.project = project;
        filter.print = Some(print);
        self.list(who, org, &filter)
            .await?
            .into_iter()
            .next()
            .ok_or(IssueError::NoSuchIssue)
    }

    pub async fn occurrences(
        &self,
        who: &Reader,
        org: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> Result<Vec<Occurrence>, IssueError> {
        self.readers.grant(who, org).await?.narrow(Some(project))?;
        Ok(self
            .reader
            .occurrences(org, project, print, window, limit.clamp(1, MAX_LIMIT))
            .await?)
    }

    pub async fn decide(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
    ) -> Result<(), IssueError> {
        let by = self.acting(token, org).await?;
        Ok(self
            .decisions
            .decide(project, print, status, by, self.now_unix_nanos())
            .await?)
    }

    pub async fn assign(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
    ) -> Result<(), IssueError> {
        let by = self.acting(token, org).await?;
        Ok(self
            .decisions
            .assign(project, print, owner, by, self.now_unix_nanos())
            .await?)
    }
}
