use super::entities::{Channel, Dispatch, Drained, Settlement};
use super::errors::{AlertError, DeliveryError};
use super::ports::{AlertDuty, AlertMailer, AlertOutbox, WebhookSender};
use super::value_objects::{Destination, Duty};
use crate::clock::UnixSeconds;

pub const RETRY_AFTER_SECS: [u64; 4] = [60, 300, 900, 3_600];
pub const DRAIN_BATCH: u32 = 50;
const MILLIS_PER_SECOND: u64 = 1_000;

pub async fn deliver<M: AlertMailer, W: WebhookSender>(
    mailer: &M,
    webhooks: &W,
    channel: &Channel,
    dispatch: &Dispatch,
) -> Result<(), DeliveryError> {
    match &channel.destination {
        Destination::Email(addresses) => mailer.send_alert(addresses, dispatch).await,
        Destination::Webhook(url) => {
            let Some(secret) = &channel.secret else {
                return Err(DeliveryError::new(
                    "the webhook carries no secret to sign with",
                ));
            };
            if !webhooks.admits(url) {
                return Err(DeliveryError::new(
                    "this deployment does not send webhooks to private addresses",
                ));
            }
            webhooks.send(url, secret, dispatch).await
        }
    }
}

pub fn settlement_after(attempts: u32, now_ms: u64, error: &DeliveryError) -> Settlement {
    let retried = usize::try_from(attempts)
        .ok()
        .and_then(|attempts| attempts.checked_sub(1))
        .and_then(|index| RETRY_AFTER_SECS.get(index));
    match retried {
        Some(after) => Settlement::Retry {
            next_at_ms: now_ms + after * MILLIS_PER_SECOND,
            error: error.0.clone(),
        },
        None => Settlement::Abandoned {
            error: error.0.clone(),
        },
    }
}

#[derive(Debug)]
pub struct Courier<'a, O, D, M, W> {
    outbox: O,
    duty: D,
    mailer: &'a M,
    webhooks: &'a W,
}

impl<'a, O, D, M, W> Courier<'a, O, D, M, W>
where
    O: AlertOutbox,
    D: AlertDuty,
    M: AlertMailer + Sync,
    W: WebhookSender + Sync,
{
    pub fn new(outbox: O, duty: D, mailer: &'a M, webhooks: &'a W) -> Self {
        Self {
            outbox,
            duty,
            mailer,
            webhooks,
        }
    }

    pub async fn drain(&self, now: UnixSeconds) -> Result<Drained, AlertError> {
        if !self.duty.lead(Duty::Deliver, now.get()).await? {
            return Ok(Drained::Skipped);
        }
        let now_ms = now.get() * MILLIS_PER_SECOND;
        let (mut sent, mut retried, mut abandoned) = (0, 0, 0);
        for claimed in self.outbox.claim(now_ms, DRAIN_BATCH).await? {
            let attempts = claimed.notification.attempt + 1;
            let settlement = match deliver(
                self.mailer,
                self.webhooks,
                &claimed.channel,
                &claimed.dispatch,
            )
            .await
            {
                Ok(()) => Settlement::Sent,
                Err(error) => settlement_after(attempts, now_ms, &error),
            };
            match settlement {
                Settlement::Sent => sent += 1,
                Settlement::Retry { .. } => retried += 1,
                Settlement::Abandoned { .. } => abandoned += 1,
            }
            self.outbox
                .settle(claimed.notification.id, attempts, &settlement)
                .await?;
        }
        Ok(Drained::Delivered {
            sent,
            retried,
            abandoned,
        })
    }
}
