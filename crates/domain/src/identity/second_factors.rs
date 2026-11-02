use super::errors::FactorError;
use super::ports::{FactorStore, SessionStore};
use super::value_objects::{
    Confirmed, DRIFT_STEPS, Enrolment, FactorKind, RecoveryCode, Standing, TotpCode, TotpSecret,
};
use crate::clock::Clock;
use crate::secret::{SecretHash, SessionToken};

const ISSUER: &str = "FerrisTrace";

#[derive(Debug)]
pub struct SecondFactors<P, S, C> {
    factors: P,
    sessions: S,
    clock: C,
}

impl<P, S, C> SecondFactors<P, S, C>
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

    pub async fn standing(&self, token: &SessionToken) -> Result<Standing, FactorError> {
        let account = self.sessions.resolve(token).await?.account;
        Ok(Standing {
            live: self.factors.live(account).await?,
            recovery_left: self.factors.recovery_left(account).await?,
        })
    }

    pub async fn enrol(
        &self,
        token: &SessionToken,
        secret: &TotpSecret,
    ) -> Result<Enrolment, FactorError> {
        let session = self.sessions.resolve(token).await?;
        let kind = FactorKind::Authenticator;
        self.factors.keep(session.account, kind, secret).await?;
        Ok(Enrolment {
            kind,
            key: secret.base32(),
            uri: secret.uri(ISSUER, &session.email),
        })
    }

    pub async fn confirm(
        &self,
        token: &SessionToken,
        code: TotpCode,
        minted: Vec<RecoveryCode>,
    ) -> Result<Confirmed, FactorError> {
        let account = self.sessions.resolve(token).await?.account;
        let kind = FactorKind::Authenticator;
        let kept = self
            .factors
            .kept(account, kind)
            .await?
            .ok_or(FactorError::NoFactor)?;
        let step = kept
            .secret
            .matched(code, self.clock.now(), DRIFT_STEPS)
            .ok_or(FactorError::Refused)?;
        let first = self.factors.live(account).await?.is_empty();
        self.factors.confirm(account, kind).await?;
        self.factors.spent_step(account, kind, step).await?;
        if !first {
            return Ok(Confirmed::Another);
        }
        let digests: Vec<SecretHash> = minted.iter().map(RecoveryCode::hash).collect();
        self.factors.keep_recovery(account, &digests).await?;
        Ok(Confirmed::First { recovery: minted })
    }

    pub async fn forget(&self, token: &SessionToken, kind: FactorKind) -> Result<(), FactorError> {
        let account = self.sessions.resolve(token).await?.account;
        if !self.factors.forget(account, kind).await? {
            return Err(FactorError::NoFactor);
        }
        if self.factors.live(account).await?.is_empty() {
            self.factors.keep_recovery(account, &[]).await?;
        }
        Ok(())
    }

    pub async fn regenerate(
        &self,
        token: &SessionToken,
        minted: Vec<RecoveryCode>,
    ) -> Result<Vec<RecoveryCode>, FactorError> {
        let account = self.sessions.resolve(token).await?.account;
        if self.factors.live(account).await?.is_empty() {
            return Err(FactorError::NoFactor);
        }
        let digests: Vec<SecretHash> = minted.iter().map(RecoveryCode::hash).collect();
        self.factors.keep_recovery(account, &digests).await?;
        Ok(minted)
    }
}
