use super::errors::ExploreError;
use super::ports::SpanReader;
use super::value_objects::{ServiceSummary, TraceFilter, TraceSummary, Window};
use crate::catalogue::ProjectId;
use crate::identity::SessionStore;
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::{Span, TenantId, TraceId};

#[derive(Debug)]
pub struct Explore<S, G, R> {
    whoami: Whoami<S, G>,
    reader: R,
}

impl<S, G, R> Explore<S, G, R>
where
    S: SessionStore,
    G: Memberships,
    R: SpanReader,
{
    pub fn new(whoami: Whoami<S, G>, reader: R) -> Self {
        Self { whoami, reader }
    }

    async fn allowed(&self, token: &SessionToken, org: TenantId) -> Result<(), ExploreError> {
        let who = self.whoami.of(token).await?;
        if who.may_act_in(org, OrgRole::Member) {
            Ok(())
        } else {
            Err(ExploreError::NotYours)
        }
    }

    pub async fn services(
        &self,
        token: &SessionToken,
        org: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Vec<ServiceSummary>, ExploreError> {
        self.allowed(token, org).await?;
        Ok(self.reader.services(org, window, project).await?)
    }

    pub async fn traces(
        &self,
        token: &SessionToken,
        org: TenantId,
        filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, ExploreError> {
        self.allowed(token, org).await?;
        Ok(self.reader.traces(org, filter).await?)
    }

    pub async fn trace(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: TraceId,
    ) -> Result<Vec<Span>, ExploreError> {
        self.allowed(token, org).await?;
        Ok(self.reader.trace(org, id).await?)
    }
}
