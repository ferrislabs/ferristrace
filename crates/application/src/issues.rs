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
    pub async fn decide_issue(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        print: Fingerprint,
        status: Status,
    ) -> Result<(), IssueError> {
        let Some(reader) = &self.issue_store else {
            return Err(IssueError::Unavailable);
        };
        Issues::new(
            Whoami::new(sessions, memberships),
            reader,
            decisions,
            &self.clock,
        )
        .decide(token, org, project, print, status)
        .await
    }

    #[transactional(sessions, memberships, decisions, unavailable = IssueError::Unavailable)]
    pub async fn assign_issue(
        &self,
        token: &SessionToken,
        org: TenantId,
        project: ProjectId,
        print: Fingerprint,
        owner: Option<AccountId>,
    ) -> Result<(), IssueError> {
        let Some(reader) = &self.issue_store else {
            return Err(IssueError::Unavailable);
        };
        Issues::new(
            Whoami::new(sessions, memberships),
            reader,
            decisions,
            &self.clock,
        )
        .assign(token, org, project, print, owner)
        .await
    }
}
