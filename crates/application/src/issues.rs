use ferristrace_domain::{
    AccountId, Fingerprint, Issue, IssueError, IssueFilter, Issues, Occurrence, ProjectId,
    SessionToken, Status, TenantId, Whoami, Window,
};

use crate::registry::IssueRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: IssueRepos,
{
    #[transactional(sessions, memberships, decisions, unavailable = IssueError::Unavailable)]
    pub async fn list_issues(
        &self,
        token: &SessionToken,
        org: TenantId,
        filter: &IssueFilter,
    ) -> Result<Vec<Issue>, IssueError> {
        let Some(reader) = &self.issue_store else {
            return Err(IssueError::Unavailable);
        };
        Issues::new(
            Whoami::new(sessions, memberships),
            reader,
            decisions,
            &self.clock,
        )
        .list(token, org, filter)
        .await
    }

    #[transactional(sessions, memberships, decisions, unavailable = IssueError::Unavailable)]
    pub async fn issue(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: Option<ProjectId>,
        print: Fingerprint,
        window: Window,
    ) -> Result<Issue, IssueError> {
        let Some(reader) = &self.issue_store else {
            return Err(IssueError::Unavailable);
        };
        Issues::new(
            Whoami::new(sessions, memberships),
            reader,
            decisions,
            &self.clock,
        )
        .one(token, org, project, print, window)
        .await
    }

    #[transactional(sessions, memberships, decisions, unavailable = IssueError::Unavailable)]
    pub async fn issue_occurrences(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> Result<Vec<Occurrence>, IssueError> {
        let Some(reader) = &self.issue_store else {
            return Err(IssueError::Unavailable);
        };
        Issues::new(
            Whoami::new(sessions, memberships),
            reader,
            decisions,
            &self.clock,
        )
        .occurrences(token, org, project, print, window, limit)
        .await
    }

    #[transactional(sessions, memberships, decisions, unavailable = IssueError::Unavailable)]
    pub async fn rule_on_issue(
        &self,
        token: &SessionToken,
        org: TenantId,
        ruling: IssueRuling,
    ) -> Result<(), IssueError> {
        let Some(reader) = &self.issue_store else {
            return Err(IssueError::Unavailable);
        };
        let issues = Issues::new(
            Whoami::new(sessions, memberships),
            reader,
            decisions,
            &self.clock,
        );
        if let Some(status) = ruling.status {
            issues
                .decide(token, org, ruling.project, ruling.print, status)
                .await?;
        }
        if let Some(owner) = ruling.owner {
            issues
                .assign(token, org, ruling.project, ruling.print, owner)
                .await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssueRuling {
    pub project: ProjectId,
    pub print: Fingerprint,
    pub status: Option<Status>,
    pub owner: Option<Option<AccountId>>,
}
