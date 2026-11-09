use super::entities::ExpiringKey;
use super::errors::KeyError;
use super::ports::ApiKeys;
use super::value_objects::{ExpiryThreshold, KEY_EXPIRY_TICK_SECS};
use crate::alerts::{AlertDuty, Duty};
use crate::audit::{Action, Actor, AuditEntry, AuditTrail, Detail, Target};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::organisation::{OrgRole, Roll};

const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revocation {
    pub key: ExpiringKey,
    pub notify: Vec<AccountId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiryNotice {
    pub key: ExpiringKey,
    pub threshold: ExpiryThreshold,
    pub notify: Vec<AccountId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expired {
    Skipped,
    Worked {
        revoked: Vec<Revocation>,
        noticed: Vec<ExpiryNotice>,
    },
}

#[derive(Debug)]
pub struct Expiry<K, R, D, A> {
    keys: K,
    roll: R,
    duty: D,
    trail: A,
}

impl<K, R, D, A> Expiry<K, R, D, A>
where
    K: ApiKeys,
    R: Roll,
    D: AlertDuty,
    A: AuditTrail,
{
    pub fn new(keys: K, roll: R, duty: D, trail: A) -> Self {
        Self {
            keys,
            roll,
            duty,
            trail,
        }
    }

    pub async fn tick(&self, now: UnixSeconds) -> Result<Expired, KeyError> {
        if !self
            .duty
            .lead(Duty::ExpireKeys, now.get() / KEY_EXPIRY_TICK_SECS)
            .await?
        {
            return Ok(Expired::Skipped);
        }
        let now_unix_nanos = now.get().saturating_mul(NANOS_PER_SECOND);

        let mut revoked = Vec::new();
        for key in self.keys.due_for_auto_revoke(now_unix_nanos).await? {
            self.keys.revoke(key.org, key.id).await?;
            self.trail
                .record(&AuditEntry {
                    org: key.org,
                    actor: Actor::System,
                    action: Action::KeyRevoked,
                    target: Target::key(key.id),
                    detail: Detail::default()
                        .with("name", &*key.name)
                        .with("reason", "expired"),
                })
                .await?;
            let notify = self.recipients_of(&key).await?;
            revoked.push(Revocation { key, notify });
        }

        let mut noticed = Vec::new();
        for (key, threshold) in self.keys.due_for_expiry_notice(now_unix_nanos).await? {
            self.keys.mark_expiry_notice_sent(key.id, threshold).await?;
            let notify = self.recipients_of(&key).await?;
            noticed.push(ExpiryNotice {
                key,
                threshold,
                notify,
            });
        }

        Ok(Expired::Worked { revoked, noticed })
    }

    async fn recipients_of(&self, key: &ExpiringKey) -> Result<Vec<AccountId>, KeyError> {
        if let Some(creator) = key.created_by {
            return Ok(vec![creator]);
        }
        Ok(self
            .roll
            .of(key.org)
            .await?
            .into_iter()
            .filter(|member| member.role.at_least(OrgRole::Admin))
            .map(|member| member.account)
            .collect())
    }
}
