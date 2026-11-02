use super::entities::{Invite, Landing, LinkNotice, Subscriber};
use super::errors::{LinkRefused, StatusError};
use super::link::{LinkKey, Purpose};
use super::ports::{StatusOutbox, StatusPages, StatusSubscribers};
use super::value_objects::{LetterId, RESEND_AFTER_SECS, Slug, StatusPageId, SubscriberId};
use crate::clock::UnixSeconds;
use crate::identity::Email;

const MILLIS_PER_SECOND: u64 = 1_000;

#[derive(Debug)]
pub struct Subscriptions<'a, Pg, Sb, O> {
    pages: Pg,
    subscribers: Sb,
    outbox: O,
    key: Option<&'a LinkKey>,
}

impl<'a, Pg, Sb, O> Subscriptions<'a, Pg, Sb, O>
where
    Pg: StatusPages,
    Sb: StatusSubscribers,
    O: StatusOutbox,
{
    pub fn new(pages: Pg, subscribers: Sb, outbox: O, key: Option<&'a LinkKey>) -> Self {
        Self {
            pages,
            subscribers,
            outbox,
            key,
        }
    }

    pub async fn subscribe(
        &self,
        slug: &Slug,
        email: &Email,
        now: UnixSeconds,
    ) -> Result<bool, StatusError> {
        if self.key.is_none() {
            return Ok(false);
        }
        let Some(page) = self.pages.by_slug(slug).await?.filter(|page| page.public) else {
            return Ok(false);
        };
        let now_ms = now.get().saturating_mul(MILLIS_PER_SECOND);
        let invited = match self.subscribers.find(page.id, email).await? {
            None => {
                let fresh = Subscriber {
                    id: SubscriberId::fresh(),
                    page: page.id,
                    email: email.clone(),
                    confirmed: false,
                    invited_ms: now_ms,
                };
                self.subscribers.add(&fresh).await?.then_some(fresh)
            }
            Some(held)
                if !held.confirmed
                    && held
                        .invited_ms
                        .saturating_add(RESEND_AFTER_SECS * MILLIS_PER_SECOND)
                        <= now_ms =>
            {
                let again = Subscriber {
                    invited_ms: now_ms,
                    ..held
                };
                self.subscribers.keep(&again).await?.then_some(again)
            }
            Some(_) => None,
        };
        let Some(invited) = invited else {
            return Ok(false);
        };
        self.outbox
            .invite(&Invite {
                id: LetterId::fresh(),
                page: page.id,
                subscriber: invited.id,
                at_ms: now_ms,
            })
            .await?;
        Ok(true)
    }

    pub async fn confirm(&self, token: &str, now: UnixSeconds) -> Result<Landing, StatusError> {
        let claim = self
            .key
            .ok_or(LinkRefused::Tampered)?
            .open(token, Purpose::Confirm, now)?;
        let held = self
            .subscribers
            .get(claim.subscriber)
            .await?
            .filter(|held| held.page == claim.page)
            .ok_or(StatusError::NoSuchSubscriber)?;
        if !held.confirmed {
            self.subscribers
                .keep(&Subscriber {
                    confirmed: true,
                    ..held
                })
                .await?;
        }
        self.landing(claim.page, LinkNotice::Confirmed).await
    }

    pub async fn unsubscribe(&self, token: &str, now: UnixSeconds) -> Result<Landing, StatusError> {
        let claim =
            self.key
                .ok_or(LinkRefused::Tampered)?
                .open(token, Purpose::Unsubscribe, now)?;
        if self
            .subscribers
            .get(claim.subscriber)
            .await?
            .is_some_and(|held| held.page == claim.page)
        {
            self.subscribers.remove(claim.subscriber).await?;
        }
        self.landing(claim.page, LinkNotice::Unsubscribed).await
    }

    async fn landing(
        &self,
        page: StatusPageId,
        notice: LinkNotice,
    ) -> Result<Landing, StatusError> {
        let page = self
            .pages
            .by_id(page)
            .await?
            .ok_or(StatusError::NoSuchPage)?;
        Ok(Landing {
            slug: page.slug,
            notice,
        })
    }
}
