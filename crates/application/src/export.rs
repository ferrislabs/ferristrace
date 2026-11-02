use ferristrace_domain::{
    Clock, ExportError, ExportGrant, Exporter, Prepared, SessionToken, SpanCursor, SpanPage,
    SpanPages, TenantId, Whoami,
};

use crate::registry::ExportRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: ExportRepos,
{
    #[transactional(sessions, memberships, export, audit, unavailable = ExportError::StoreUnreachable)]
    pub async fn begin_export(
        &self,
        token: &SessionToken,
        org: TenantId,
    ) -> Result<Prepared, ExportError> {
        if self.span_store.is_none() {
            return Err(ExportError::StoreUnreachable);
        }
        Exporter::new(Whoami::new(sessions, memberships), export, audit)
            .begin(token, org, self.clock.now())
            .await
    }
}

impl<S: Stack> Ferristrace<S> {
    pub async fn export_span_page(
        &self,
        grant: ExportGrant,
        after: Option<&SpanCursor>,
        limit: u32,
    ) -> Result<SpanPage, ExportError> {
        let Some(store) = &self.span_store else {
            return Err(ExportError::StoreUnreachable);
        };
        Ok(store.page(grant.org(), after, limit).await?)
    }
}
