use ferristrace_domain::{
    CallPage, CallQuery, Llm, LlmError, Price, PriceDraft, PriceId, Reader, Readers, SessionToken,
    Spend, SpendQuery, TenantId, Unpriced, Whoami, Window,
};

use crate::registry::LlmRepos;
use crate::{Ferristrace, Stack, transactional};

impl<S: Stack> Ferristrace<S>
where
    S::Repos: LlmRepos,
{
    #[transactional(sessions, memberships, api_keys, model_prices, audit, unavailable = LlmError::StoreUnreachable)]
    pub async fn llm_calls(
        &self,
        who: &Reader,
        org: TenantId,
        query: &CallQuery,
    ) -> Result<CallPage, LlmError> {
        let Some(reader) = &self.issue_store else {
            return Err(LlmError::StoreUnreachable);
        };
        Llm::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            model_prices,
            audit,
        )
        .calls(who, org, query, reader)
        .await
    }

    #[transactional(sessions, memberships, api_keys, model_prices, audit, unavailable = LlmError::StoreUnreachable)]
    pub async fn llm_spend(
        &self,
        who: &Reader,
        org: TenantId,
        query: &SpendQuery,
    ) -> Result<Spend, LlmError> {
        let Some(reader) = &self.issue_store else {
            return Err(LlmError::StoreUnreachable);
        };
        Llm::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            model_prices,
            audit,
        )
        .spend(who, org, query, reader)
        .await
    }

    #[transactional(sessions, memberships, api_keys, model_prices, audit, unavailable = LlmError::StoreUnreachable)]
    pub async fn llm_unpriced(
        &self,
        who: &Reader,
        org: TenantId,
        window: Window,
    ) -> Result<Vec<Unpriced>, LlmError> {
        let Some(reader) = &self.issue_store else {
            return Err(LlmError::StoreUnreachable);
        };
        Llm::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            model_prices,
            audit,
        )
        .unpriced(who, org, window, reader)
        .await
    }

    #[transactional(sessions, memberships, api_keys, model_prices, audit, unavailable = LlmError::StoreUnreachable)]
    pub async fn llm_prices(&self, who: &Reader, org: TenantId) -> Result<Vec<Price>, LlmError> {
        Llm::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            model_prices,
            audit,
        )
        .list(who, org)
        .await
    }

    #[transactional(sessions, memberships, api_keys, model_prices, audit, unavailable = LlmError::StoreUnreachable)]
    pub async fn add_llm_price(
        &self,
        token: &SessionToken,
        org: TenantId,
        draft: &PriceDraft,
    ) -> Result<Price, LlmError> {
        Llm::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            model_prices,
            audit,
        )
        .add(token, org, draft)
        .await
    }

    #[transactional(sessions, memberships, api_keys, model_prices, audit, unavailable = LlmError::StoreUnreachable)]
    pub async fn remove_llm_price(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: PriceId,
    ) -> Result<(), LlmError> {
        Llm::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            model_prices,
            audit,
        )
        .remove(token, org, id)
        .await
    }
}
