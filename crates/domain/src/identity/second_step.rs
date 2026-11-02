use super::errors::FactorError;
use super::ports::{FactorStore, SessionStore};
use super::value_objects::{AccountId, Agent, Answer, ChallengeToken, DRIFT_STEPS, FactorKind};
use crate::clock::Clock;
use crate::secret::SessionToken;

pub const MAX_ANSWERS: u32 = 5;

#[derive(Debug)]
pub struct SecondStep<P, S, C> {
    factors: P,
    sessions: S,
    clock: C,
}

impl<P, S, C> SecondStep<P, S, C>
where
    P: FactorStore,
    S: SessionStore,
    C: Clock,
{
    pub fn new(factors: P, sessions: S, clock: C) -> Self {
        Self {
            factors,
            sessions,
            clock,
        }
    }

    pub async fn verify(
        &self,
        challenge: &ChallengeToken,
        answer: &Answer,
        from: Option<&Agent>,
    ) -> Result<SessionToken, FactorError> {
        let digest = challenge.hash();
        let pending = self.factors.answered(digest).await?;
        if pending.answers > MAX_ANSWERS {
            self.factors.close(digest).await?;
            return Err(FactorError::Exhausted);
        }
        if !self.admits(pending.account, answer).await? {
            return Err(FactorError::Refused);
        }
        self.factors.close(digest).await?;
        Ok(self.sessions.issue(pending.account, None, from).await?)
    }

    async fn admits(&self, account: AccountId, answer: &Answer) -> Result<bool, FactorError> {
        match answer {
            Answer::Code(code) => {
                let kind = FactorKind::Authenticator;
                let Some(kept) = self.factors.kept(account, kind).await? else {
                    return Ok(false);
                };
                if !kept.confirmed {
                    return Ok(false);
                }
                let Some(step) = kept.secret.matched(*code, self.clock.now(), DRIFT_STEPS) else {
                    return Ok(false);
                };
                if kept.last_step.is_some_and(|last| step <= last) {
                    return Ok(false);
                }
                self.factors.spent_step(account, kind, step).await?;
                Ok(true)
            }
            Answer::Recovery(code) => self.factors.spend_recovery(account, code.hash()).await,
        }
    }
}
