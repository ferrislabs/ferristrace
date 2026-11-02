use std::collections::BTreeMap;

use super::entities::{CallPage, Price, PricedCall, Unpriced};
use super::errors::LlmError;
use super::ports::{LlmReader, ModelPrices};
use super::pricing::PriceBook;
use super::value_objects::{CallQuery, PriceDraft, PriceId};
use crate::access::{ApiKeys, Reader, Readers};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::explore::Window;
use crate::identity::{AccountId, SessionStore};
use crate::organisation::{Memberships, OrgRole};
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct Llm<S, G, K, P, A> {
    readers: Readers<S, G, K>,
    prices: P,
    trail: A,
}

impl<S, G, K, P, A> Llm<S, G, K, P, A>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    P: ModelPrices,
    A: AuditTrail,
{
    pub fn new(readers: Readers<S, G, K>, prices: P, trail: A) -> Self {
        Self {
            readers,
            prices,
            trail,
        }
    }

    pub async fn calls<R: LlmReader>(
        &self,
        who: &Reader,
        org: TenantId,
        query: &CallQuery,
        reader: &R,
    ) -> Result<CallPage, LlmError> {
        let project = self.readers.grant(who, org).await?.narrow(query.project)?;
        let prices = self.prices.list(org).await?;
        let page = if project == query.project {
            reader.calls(org, query).await?
        } else {
            let mut narrowed = query.clone();
            narrowed.project = project;
            reader.calls(org, &narrowed).await?
        };
        let book = PriceBook::new(&prices);
        Ok(CallPage {
            calls: page
                .calls
                .into_iter()
                .map(|priced| PricedCall {
                    cost: book.cost(&priced.call),
                    call: priced.call,
                })
                .collect(),
            next: page.next,
        })
    }

    pub async fn unpriced<R: LlmReader>(
        &self,
        who: &Reader,
        org: TenantId,
        window: Window,
        reader: &R,
    ) -> Result<Vec<Unpriced>, LlmError> {
        let project = self.readers.grant(who, org).await?.narrow(None)?;
        let prices = self.prices.list(org).await?;
        let book = PriceBook::new(&prices);
        let mut grouped: BTreeMap<(Box<str>, Box<str>), Unpriced> = BTreeMap::new();
        for seen in reader.models(org, window, project).await? {
            let model: Box<str> = if seen.response_model.is_empty() {
                seen.request_model.clone()
            } else {
                seen.response_model.clone()
            };
            let at_ms = seen.last_unix_nanos / 1_000_000;
            let found = [&seen.response_model, &seen.request_model]
                .into_iter()
                .filter(|held| !held.is_empty())
                .any(|held| book.find(&seen.provider, held, at_ms).is_some());
            if found {
                continue;
            }
            let entry = grouped
                .entry((seen.provider.clone(), model.clone()))
                .or_insert_with(|| Unpriced {
                    provider: seen.provider.clone(),
                    model,
                    calls: 0,
                    last_unix_nanos: 0,
                });
            entry.calls += seen.calls;
            entry.last_unix_nanos = entry.last_unix_nanos.max(seen.last_unix_nanos);
        }
        let mut listed: Vec<Unpriced> = grouped.into_values().collect();
        listed.sort_by(|left, right| {
            right
                .calls
                .cmp(&left.calls)
                .then_with(|| left.model.cmp(&right.model))
        });
        Ok(listed)
    }

    pub async fn list(&self, who: &Reader, org: TenantId) -> Result<Vec<Price>, LlmError> {
        self.readers.grant(who, org).await?;
        Ok(self.prices.list(org).await?)
    }

    pub async fn add(
        &self,
        token: &SessionToken,
        org: TenantId,
        draft: &PriceDraft,
    ) -> Result<Price, LlmError> {
        let by = self.standing(token, org).await?;
        let price = self
            .prices
            .create(org, draft, by)
            .await?
            .ok_or(LlmError::PriceTaken)?;
        self.note(org, by, Action::PriceCreated, &price).await?;
        Ok(price)
    }

    pub async fn remove(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: PriceId,
    ) -> Result<(), LlmError> {
        let by = self.standing(token, org).await?;
        let price = self
            .prices
            .delete(org, id)
            .await?
            .ok_or(LlmError::NoSuchPrice)?;
        self.note(org, by, Action::PriceDeleted, &price).await
    }

    async fn standing(&self, token: &SessionToken, org: TenantId) -> Result<AccountId, LlmError> {
        let who = self.readers.whoami().of(token).await?;
        match who.role_in(org) {
            None => Err(LlmError::NotYours),
            Some(role) if !role.at_least(OrgRole::Admin) => Err(LlmError::NotYoursToChange),
            Some(_) => Ok(who.account),
        }
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        price: &Price,
    ) -> Result<(), LlmError> {
        let detail = Detail::default()
            .with("provider", &price.provider)
            .with("model", &price.model)
            .with("input", price.rates.input)
            .with("output", price.rates.output)
            .with_maybe("cached_input", price.rates.cached_input)
            .with("effective_from_ms", price.effective_from_ms);
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(by),
                action,
                target: Target::of(TargetKind::Price, Some(price.id.to_string().into())),
                detail,
            })
            .await?)
    }
}
