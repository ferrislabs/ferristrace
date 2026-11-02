use super::entities::{Heralded, Letter, LetterBody};
use super::errors::StatusError;
use super::link::{LinkClaim, LinkKey, Purpose};
use super::ports::{StatusMailer, StatusOutbox};
use super::value_objects::{ANNOUNCE_TICK_SECS, CONFIRM_LIFETIME_SECS, UNSUBSCRIBE_LIFETIME_SECS};
use crate::alerts::{AlertDuty, DRAIN_BATCH, Duty, Settlement, settlement_after};
use crate::clock::UnixSeconds;

const MILLIS_PER_SECOND: u64 = 1_000;
const NO_KEY: &str = "no link key is configured, so no subscriber email can carry its links";

#[derive(Debug)]
pub struct Herald<'a, O, D, M> {
    outbox: O,
    duty: D,
    mailer: &'a M,
    key: Option<&'a LinkKey>,
}

impl<'a, O, D, M> Herald<'a, O, D, M>
where
    O: StatusOutbox,
    D: AlertDuty,
    M: StatusMailer + Sync,
{
    pub fn new(outbox: O, duty: D, mailer: &'a M, key: Option<&'a LinkKey>) -> Self {
        Self {
            outbox,
            duty,
            mailer,
            key,
        }
    }

    pub async fn tick(&self, now: UnixSeconds) -> Result<Heralded, StatusError> {
        if !self
            .duty
            .lead(Duty::Announce, now.get() / ANNOUNCE_TICK_SECS)
            .await?
        {
            return Ok(Heralded::Skipped);
        }
        let now_ms = now.get().saturating_mul(MILLIS_PER_SECOND);
        let (mut sent, mut retried, mut abandoned) = (0, 0, 0);
        for claimed in self.outbox.claim(now_ms, DRAIN_BATCH).await? {
            let attempts = claimed.attempt + 1;
            let settlement = match self.key {
                None => Settlement::Abandoned {
                    error: NO_KEY.into(),
                },
                Some(key) => {
                    let sign = |purpose, lifetime| {
                        key.sign(
                            &LinkClaim {
                                purpose,
                                page: claimed.page,
                                subscriber: claimed.subscriber,
                            },
                            now.saturating_add_secs(lifetime),
                        )
                    };
                    let letter = Letter {
                        page_name: claimed.page_name.clone(),
                        slug: claimed.slug.clone(),
                        confirm: matches!(claimed.body, LetterBody::Confirm)
                            .then(|| sign(Purpose::Confirm, CONFIRM_LIFETIME_SECS)),
                        unsubscribe: sign(Purpose::Unsubscribe, UNSUBSCRIBE_LIFETIME_SECS),
                        body: claimed.body.clone(),
                    };
                    match self.mailer.send_status(&claimed.email, &letter).await {
                        Ok(()) => Settlement::Sent,
                        Err(error) => settlement_after(attempts, now_ms, &error),
                    }
                }
            };
            match settlement {
                Settlement::Sent => sent += 1,
                Settlement::Retry { .. } => retried += 1,
                Settlement::Abandoned { .. } => abandoned += 1,
            }
            self.outbox
                .settle(claimed.id, attempts, &settlement)
                .await?;
        }
        Ok(Heralded::Worked {
            sent,
            retried,
            abandoned,
        })
    }
}
