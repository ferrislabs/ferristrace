use ferristrace_domain::{
    AccountId, Fingerprint, Issue, IssueError, IssueFilter, Issues, Occurrence, ProjectId, Reader,
    Readers, SessionToken, Status, TenantId, Whoami, Window,
};

use crate::registry::IssueRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: IssueRepos,
{
    #[transactional(sessions, memberships, api_keys, decisions, unavailable = IssueError::Unavailable)]
    pub async fn list_issues(
        &self,
        who: &Reader,
        org: TenantId,
        filter: &IssueFilter,
    ) -> Result<Vec<Issue>, IssueError> {
        let Some(reader) = &self.issue_store else {
            return Err(IssueError::Unavailable);
        };
        Issues::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
            decisions,
            &self.clock,
        )
        .list(who, org, filter)
        .await
    }

    #[transactional(sessions, memberships, api_keys, decisions, unavailable = IssueError::Unavailable)]
    pub async fn issue(
        &self,
        who: &Reader,
        org: TenantId,
        project: Option<ProjectId>,
        print: Fingerprint,
        window: Window,
    ) -> Result<Issue, IssueError> {
        let Some(reader) = &self.issue_store else {
            return Err(IssueError::Unavailable);
        };
        Issues::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
            decisions,
            &self.clock,
        )
        .one(who, org, project, print, window)
        .await
    }

    #[transactional(sessions, memberships, api_keys, decisions, unavailable = IssueError::Unavailable)]
    pub async fn issue_occurrences(
        &self,
        who: &Reader,
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
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            reader,
            decisions,
            &self.clock,
        )
        .occurrences(who, org, project, print, window, limit)
        .await
    }

    #[transactional(sessions, memberships, api_keys, decisions, unavailable = IssueError::Unavailable)]
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
            Readers::new(Whoami::new(sessions, memberships), api_keys),
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
