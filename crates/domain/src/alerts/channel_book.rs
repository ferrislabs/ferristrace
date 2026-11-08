use super::alert_book::standing;
use super::entities::{ChannelView, IssuedChannel};
use super::errors::{AlertError, BadChannel};
use super::ports::{AlertChannels, WebhookSender};
use super::value_objects::{ChannelId, ChannelName, Destination, WebhookSecret};
use crate::access::{ApiKeys, Reader, Readers};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target, TargetKind};
use crate::identity::{AccountId, SessionStore};
use crate::organisation::Memberships;
use crate::secret::SessionToken;
use crate::telemetry::TenantId;

#[derive(Debug)]
pub struct ChannelBook<S, G, K, C, A> {
    readers: Readers<S, G, K>,
    channels: C,
    trail: A,
}

impl<S, G, K, C, A> ChannelBook<S, G, K, C, A>
where
    S: SessionStore,
    G: Memberships,
    K: ApiKeys,
    C: AlertChannels,
    A: AuditTrail,
{
    pub fn new(readers: Readers<S, G, K>, channels: C, trail: A) -> Self {
        Self {
            readers,
            channels,
            trail,
        }
    }

    pub async fn list(&self, who: &Reader, org: TenantId) -> Result<Vec<ChannelView>, AlertError> {
        self.readers.grant(who, org).await?;
        Ok(self.channels.list(org).await?)
    }

    pub async fn create<W: WebhookSender>(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &ChannelName,
        destination: &Destination,
        webhooks: &W,
    ) -> Result<IssuedChannel, AlertError> {
        let by = standing(self.readers.whoami(), token, org).await?;
        let secret = match destination {
            Destination::Email(_) => None,
            Destination::Chat(_, url) => {
                if !webhooks.admits(url) {
                    return Err(AlertError::PrivateTarget);
                }
                None
            }
            Destination::Webhook(url) => {
                if !webhooks.admits(url) {
                    return Err(AlertError::PrivateTarget);
                }
                if !self.channels.keeps_secrets() {
                    return Err(AlertError::NoSecretKeeper);
                }
                Some(WebhookSecret::generate())
            }
        };
        let channel = self
            .channels
            .create(org, name, destination, secret.as_ref(), by)
            .await?
            .ok_or(AlertError::NameTaken)?;
        self.note(
            org,
            by,
            Action::ChannelCreated,
            channel.id,
            Detail::default()
                .with("name", name)
                .with("kind", destination.kind()),
        )
        .await?;
        Ok(IssuedChannel { channel, secret })
    }

    pub async fn update<W: WebhookSender>(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ChannelId,
        name: Option<&ChannelName>,
        target: Option<&str>,
        webhooks: &W,
    ) -> Result<ChannelView, AlertError> {
        let by = standing(self.readers.whoami(), token, org).await?;
        let held = self.channels.list(org).await?;
        let kept = held
            .iter()
            .find(|channel| channel.id == id)
            .ok_or(AlertError::NoSuchChannel)?;
        let name = name.unwrap_or(&kept.name);
        if held
            .iter()
            .any(|channel| channel.id != id && channel.name == *name)
        {
            return Err(AlertError::NameTaken);
        }
        let destination = match target {
            Some(target) => Destination::parse(kept.kind(), target)?,
            None => kept.destination.clone(),
        };
        if destination.kind() != kept.kind() {
            return Err(BadChannel::KindChanged.into());
        }
        if let Destination::Webhook(url) | Destination::Chat(_, url) = &destination
            && !webhooks.admits(url)
        {
            return Err(AlertError::PrivateTarget);
        }
        if !self.channels.update(org, id, name, &destination).await? {
            return Err(AlertError::NoSuchChannel);
        }
        self.note(
            org,
            by,
            Action::ChannelUpdated,
            id,
            Detail::default().with("name", name),
        )
        .await?;
        Ok(ChannelView {
            id,
            name: name.clone(),
            destination,
            has_secret: kept.has_secret,
        })
    }

    pub async fn delete(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ChannelId,
    ) -> Result<(), AlertError> {
        let by = standing(self.readers.whoami(), token, org).await?;
        let name = self
            .channels
            .list(org)
            .await?
            .into_iter()
            .find(|channel| channel.id == id)
            .map(|channel| channel.name)
            .ok_or(AlertError::NoSuchChannel)?;
        if !self.channels.delete(org, id).await? {
            return Err(AlertError::NoSuchChannel);
        }
        self.note(
            org,
            by,
            Action::ChannelDeleted,
            id,
            Detail::default().with("name", name),
        )
        .await
    }

    async fn note(
        &self,
        org: TenantId,
        by: AccountId,
        action: Action,
        channel: ChannelId,
        detail: Detail,
    ) -> Result<(), AlertError> {
        Ok(self
            .trail
            .record(&AuditEntry {
                org,
                actor: Actor::Account(by),
                action,
                target: Target::of(TargetKind::Channel, Some(channel.to_string().into())),
                detail,
            })
            .await?)
    }
}
