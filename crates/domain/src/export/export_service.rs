use super::entities::{ExportGrant, Prepared};
use super::errors::ExportError;
use super::ports::ExportSource;
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target};
use crate::clock::UnixSeconds;
use crate::identity::SessionStore;
use crate::organisation::{Memberships, OrgRole, Whoami};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Exporter<S, G, X, A> {
    whoami: Whoami<S, G>,
    source: X,
    trail: A,
}

impl<S, G, X, A> Exporter<S, G, X, A>
where
    S: SessionStore,
    G: Memberships,
    X: ExportSource,
    A: AuditTrail,
{
    pub fn new(whoami: Whoami<S, G>, source: X, trail: A) -> Self {
        Self {
            whoami,
            source,
            trail,
        }
    }

    pub async fn begin(
        &self,
        token: &SessionToken,
        org: TenantId,
        at: UnixSeconds,
    ) -> Result<Prepared, ExportError> {
        let who = self.whoami.of(token).await?;
        match who.role_in(org) {
            None => return Err(ExportError::NotYours),
            Some(role) if !role.at_least(OrgRole::Owner) => return Err(ExportError::OwnersOnly),
            Some(_) => {}
        }
        self.trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(who.account),
                action: Action::OrgExported,
                target: Target::org(org),
                detail: Detail::default(),
            })
            .await?;
        let snapshot = self.source.snapshot(org).await?;
        Ok(Prepared {
            grant: ExportGrant::of(org),
            exported_at: at,
            snapshot,
        })
    }
}
