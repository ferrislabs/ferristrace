use ferristrace_domain::{
    AuditError, AuditFilter, AuditLog, AuditPage, PageSize, SessionToken, TenantId, Whoami,
};

use crate::registry::AuditRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: AuditRepos,
{
    #[transactional(sessions, memberships, audit, unavailable = AuditError::StoreUnreachable)]
    pub async fn audit_log(
        &self,
        token: &SessionToken,
        org: TenantId,
        filter: &AuditFilter,
        page: PageSize,
    ) -> Result<AuditPage, AuditError> {
        AuditLog::new(Whoami::new(sessions, memberships), audit)
            .read(token, org, filter, page)
            .await
    }
}
