use super::entities::AuditPage;
use super::errors::AuditError;
use super::ports::AuditTrail;
use super::value_objects::{AuditFilter, PageSize};
use crate::identity::SessionStore;
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct AuditLog<S, G, A> {
    whoami: Whoami<S, G>,
    trail: A,
}

impl<S, G, A> AuditLog<S, G, A>
where
    S: SessionStore,
    G: Memberships,
    A: AuditTrail,
{
    pub fn new(whoami: Whoami<S, G>, trail: A) -> Self {
        Self { whoami, trail }
    }

    pub async fn read(
        &self,
        token: &SessionToken,
        org: TenantId,
        filter: &AuditFilter,
        page: PageSize,
    ) -> Result<AuditPage, AuditError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => return Err(AuditError::NotYours),
            Some(role) if !role.at_least(OrgRole::Admin) => {
                return Err(AuditError::NotYoursToRead);
            }
            Some(_) => {}
        }
        Ok(self.trail.list(org, filter, page).await?)
    }
}
