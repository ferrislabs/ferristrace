use core::future::Future;

use super::entities::{Agent, AgentEnrolment};
use super::errors::TokenRefusal;
use super::value_objects::{AgentId, AgentToken, VerifiedToken};
use crate::identity::AccountId;
use crate::store::StoreError;
use crate::telemetry::TenantId;

pub trait Agents {
    fn list(&self, org: TenantId) -> impl Future<Output = Result<Vec<Agent>, StoreError>> + Send;

    fn register(
        &self,
        org: TenantId,
        enrolment: &AgentEnrolment,
        by: AccountId,
    ) -> impl Future<Output = Result<Option<Agent>, StoreError>> + Send;

    fn revoke(
        &self,
        org: TenantId,
        id: AgentId,
    ) -> impl Future<Output = Result<Option<Agent>, StoreError>> + Send;
}

pub trait AgentTokens {
    fn verify(
        &self,
        token: &AgentToken,
    ) -> impl Future<Output = Result<VerifiedToken, TokenRefusal>> + Send;
}

impl<T: AgentTokens + Sync> AgentTokens for &T {
    fn verify(
        &self,
        token: &AgentToken,
    ) -> impl Future<Output = Result<VerifiedToken, TokenRefusal>> + Send {
        (*self).verify(token)
    }
}

impl<T: AgentTokens + Send + Sync> AgentTokens for std::sync::Arc<T> {
    fn verify(
        &self,
        token: &AgentToken,
    ) -> impl Future<Output = Result<VerifiedToken, TokenRefusal>> + Send {
        self.as_ref().verify(token)
    }
}
