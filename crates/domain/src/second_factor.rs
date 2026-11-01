use core::fmt;
use core::future::Future;

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::clock::Clock;
use crate::identity::AccountId;
use crate::secret::{SecretHash, SessionToken};
use crate::session::{Agent, SessionError, SessionStore};
use crate::totp::{Step, TotpCode, TotpSecret};

pub const CHALLENGE_PREFIX: &str = "ct_2f_";
pub const RECOVERY_CODES: usize = 10;
pub const MAX_ANSWERS: u32 = 5;

const ISSUER: &str = "Comettrace";
const DRIFT_STEPS: u8 = 1;
const TOKEN_BYTES: usize = 32;
const GROUP: usize = 5;
const RECOVERY_ALPHABET: &[u8; 31] = b"abcdefghjkmnpqrstuvwxyz23456789";
const RECOVERY_SPAN: u8 = 31;
const RECOVERY_CEILING: u8 = RECOVERY_SPAN * 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FactorError {
    #[error("no live session carries this token")]
    NoSession,
    #[error("no live challenge carries this token")]
    NoChallenge,
    #[error("that answer opens nothing")]
    Refused,
    #[error("this challenge has taken all the answers it will take")]
    Exhausted,
    #[error("this account keeps no such factor")]
    NoFactor,
    #[error("this account already keeps that factor")]
    AlreadyKept,
    #[error("this deployment cannot offer a second factor")]
    NotOffered,
    #[error("the control plane could not be reached")]
    StoreUnreachable,
}

impl From<SessionError> for FactorError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unknown => Self::NoSession,
            SessionError::StoreUnreachable => Self::StoreUnreachable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactorKind {
    Authenticator,
}

impl FactorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authenticator => "app",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "app" => Some(Self::Authenticator),
            _ => None,
        }
    }
}

impl fmt::Display for FactorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct ChallengeToken(String);

impl ChallengeToken {
    pub fn random() -> Self {
        let bytes: [u8; TOKEN_BYTES] = rand::random();
        Self(format!("{CHALLENGE_PREFIX}{}", hex::encode(bytes)))
    }

    pub fn from_plaintext(plaintext: &str) -> Self {
        Self(plaintext.to_owned())
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> SecretHash {
        SecretHash::of(&self.0)
    }

    pub fn looks_like_one(&self) -> bool {
        self.0.starts_with(CHALLENGE_PREFIX) && self.0.len() > CHALLENGE_PREFIX.len()
    }
}

impl fmt::Debug for ChallengeToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ChallengeToken(redacted)")
    }
}

#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct RecoveryCode(String);

impl RecoveryCode {
    pub fn random() -> Self {
        let mut rendered = String::with_capacity(GROUP * 2 + 1);
        for place in 0..GROUP * 2 {
            if place == GROUP {
                rendered.push('-');
            }
            rendered.push(symbol());
        }
        Self(rendered)
    }

    pub fn ten() -> Vec<Self> {
        (0..RECOVERY_CODES).map(|_| Self::random()).collect()
    }

    pub fn from_plaintext(plaintext: &str) -> Self {
        let kept: Vec<u8> = plaintext
            .bytes()
            .map(|byte| byte.to_ascii_lowercase())
            .filter(|byte| RECOVERY_ALPHABET.contains(byte))
            .collect();
        if kept.len() != GROUP * 2 {
            return Self(String::new());
        }
        let (head, tail) = kept.split_at(GROUP);
        let mut rendered = String::with_capacity(GROUP * 2 + 1);
        rendered.push_str(&String::from_utf8_lossy(head));
        rendered.push('-');
        rendered.push_str(&String::from_utf8_lossy(tail));
        Self(rendered)
    }

    pub fn exposed(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> SecretHash {
        SecretHash::of(&self.0)
    }

    pub fn looks_like_one(&self) -> bool {
        self.0.len() == GROUP * 2 + 1
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryCode(redacted)")
    }
}

fn symbol() -> char {
    loop {
        let drawn: u8 = rand::random();
        if drawn < RECOVERY_CEILING {
            return char::from(RECOVERY_ALPHABET[usize::from(drawn % RECOVERY_SPAN)]);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub live: Vec<FactorKind>,
    pub recovery_left: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrolment {
    pub kind: FactorKind,
    pub key: String,
    pub uri: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Confirmed {
    First { recovery: Vec<RecoveryCode> },
    Another,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Code(TotpCode),
    Recovery(RecoveryCode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending {
    pub account: AccountId,
    pub answers: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub secret: TotpSecret,
    pub confirmed: bool,
    pub last_step: Option<Step>,
}

pub trait Gate {
    fn offered(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<FactorKind>, FactorError>> + Send;

    fn open(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<(), FactorError>> + Send;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoSecondFactor;

impl Gate for NoSecondFactor {
    async fn offered(&self, _account: AccountId) -> Result<Vec<FactorKind>, FactorError> {
        Ok(Vec::new())
    }

    async fn open(&self, _account: AccountId, _digest: SecretHash) -> Result<(), FactorError> {
        Err(FactorError::NotOffered)
    }
}

pub trait FactorStore {
    fn live(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<FactorKind>, FactorError>> + Send;

    fn keep(
        &self,
        account: AccountId,
        kind: FactorKind,
        secret: &TotpSecret,
    ) -> impl Future<Output = Result<(), FactorError>> + Send;

    fn kept(
        &self,
        account: AccountId,
        kind: FactorKind,
    ) -> impl Future<Output = Result<Option<Kept>, FactorError>> + Send;

    fn confirm(
        &self,
        account: AccountId,
        kind: FactorKind,
    ) -> impl Future<Output = Result<(), FactorError>> + Send;

    fn forget(
        &self,
        account: AccountId,
        kind: FactorKind,
    ) -> impl Future<Output = Result<bool, FactorError>> + Send;

    fn spent_step(
        &self,
        account: AccountId,
        kind: FactorKind,
        step: Step,
    ) -> impl Future<Output = Result<(), FactorError>> + Send;

    fn keep_recovery(
        &self,
        account: AccountId,
        digests: &[SecretHash],
    ) -> impl Future<Output = Result<(), FactorError>> + Send;

    fn spend_recovery(
        &self,
        account: AccountId,
        digest: SecretHash,
    ) -> impl Future<Output = Result<bool, FactorError>> + Send;

    fn recovery_left(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<u32, FactorError>> + Send;

    fn answered(
        &self,
        digest: SecretHash,
    ) -> impl Future<Output = Result<Pending, FactorError>> + Send;

    fn close(&self, digest: SecretHash) -> impl Future<Output = Result<(), FactorError>> + Send;
}

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::{
        Answer, ChallengeToken, Confirmed, FactorError, FactorKind, FactorStore, Kept, MAX_ANSWERS,
        Pending, RECOVERY_ALPHABET, RECOVERY_CODES, RECOVERY_SPAN, RecoveryCode, SecondFactors,
        SecondStep, Standing,
    };
    use crate::clock::{Clock, UnixSeconds};
    use crate::credential::Email;
    use crate::identity::AccountId;
    use crate::secret::{SecretHash, SessionToken};
    use crate::session::{Agent, Session, SessionError, SessionStore};
    use crate::tenant::TenantId;
    use crate::totp::{Step, TotpCode, TotpSecret};

    const NOW: u64 = 1_700_000_000;

    struct At(u64);

    impl Clock for At {
        fn now(&self) -> UnixSeconds {
            UnixSeconds::new(self.0)
        }
    }

    #[derive(Default)]
    struct Held {
        factors: HashMap<FactorKind, Kept>,
        recovery: Vec<SecretHash>,
        challenges: HashMap<[u8; 32], u32>,
    }

    #[derive(Default)]
    struct Store(Mutex<Held>);

    impl Store {
        fn with_factor(secret: &TotpSecret, confirmed: bool) -> Self {
            let store = Self::default();
            store.0.lock().expect("the lock").factors.insert(
                FactorKind::Authenticator,
                Kept {
                    secret: secret.clone(),
                    confirmed,
                    last_step: None,
                },
            );
            store
        }

        fn challenging(&self, digest: SecretHash) {
            self.0
                .lock()
                .expect("the lock")
                .challenges
                .insert(*digest.as_bytes(), 0);
        }

        fn holding(&self, codes: &[RecoveryCode]) {
            self.0.lock().expect("the lock").recovery =
                codes.iter().map(RecoveryCode::hash).collect();
        }

        fn live_challenges(&self) -> usize {
            self.0.lock().expect("the lock").challenges.len()
        }

        fn recovery_held(&self) -> usize {
            self.0.lock().expect("the lock").recovery.len()
        }

        fn still_holds(&self, code: &RecoveryCode) -> bool {
            self.0
                .lock()
                .expect("the lock")
                .recovery
                .contains(&code.hash())
        }

        fn confirmed(&self) -> bool {
            self.0
                .lock()
                .expect("the lock")
                .factors
                .get(&FactorKind::Authenticator)
                .is_some_and(|kept| kept.confirmed)
        }
    }

    impl FactorStore for &Store {
        async fn live(&self, _account: AccountId) -> Result<Vec<FactorKind>, FactorError> {
            Ok(self
                .0
                .lock()
                .expect("the lock")
                .factors
                .iter()
                .filter(|(_, kept)| kept.confirmed)
                .map(|(kind, _)| *kind)
                .collect())
        }

        async fn keep(
            &self,
            _account: AccountId,
            kind: FactorKind,
            secret: &TotpSecret,
        ) -> Result<(), FactorError> {
            self.0.lock().expect("the lock").factors.insert(
                kind,
                Kept {
                    secret: secret.clone(),
                    confirmed: false,
                    last_step: None,
                },
            );
            Ok(())
        }

        async fn kept(
            &self,
            _account: AccountId,
            kind: FactorKind,
        ) -> Result<Option<Kept>, FactorError> {
            Ok(self.0.lock().expect("the lock").factors.get(&kind).cloned())
        }

        async fn confirm(&self, _account: AccountId, kind: FactorKind) -> Result<(), FactorError> {
            let mut held = self.0.lock().expect("the lock");
            let kept = held.factors.get_mut(&kind).ok_or(FactorError::NoFactor)?;
            kept.confirmed = true;
            Ok(())
        }

        async fn forget(&self, _account: AccountId, kind: FactorKind) -> Result<bool, FactorError> {
            Ok(self
                .0
                .lock()
                .expect("the lock")
                .factors
                .remove(&kind)
                .is_some())
        }

        async fn spent_step(
            &self,
            _account: AccountId,
            kind: FactorKind,
            step: Step,
        ) -> Result<(), FactorError> {
            let mut held = self.0.lock().expect("the lock");
            let kept = held.factors.get_mut(&kind).ok_or(FactorError::NoFactor)?;
            kept.last_step = Some(step);
            Ok(())
        }

        async fn keep_recovery(
            &self,
            _account: AccountId,
            digests: &[SecretHash],
        ) -> Result<(), FactorError> {
            self.0.lock().expect("the lock").recovery = digests.to_vec();
            Ok(())
        }

        async fn spend_recovery(
            &self,
            _account: AccountId,
            digest: SecretHash,
        ) -> Result<bool, FactorError> {
            let mut held = self.0.lock().expect("the lock");
            let Some(at) = held.recovery.iter().position(|kept| *kept == digest) else {
                return Ok(false);
            };
            held.recovery.remove(at);
            Ok(true)
        }

        async fn recovery_left(&self, _account: AccountId) -> Result<u32, FactorError> {
            Ok(u32::try_from(self.0.lock().expect("the lock").recovery.len()).unwrap_or(u32::MAX))
        }

        async fn answered(&self, digest: SecretHash) -> Result<Pending, FactorError> {
            let mut held = self.0.lock().expect("the lock");
            let answers = held
                .challenges
                .get_mut(digest.as_bytes())
                .ok_or(FactorError::NoChallenge)?;
            *answers += 1;
            Ok(Pending {
                account: an_account(),
                answers: *answers,
            })
        }

        async fn close(&self, digest: SecretHash) -> Result<(), FactorError> {
            self.0
                .lock()
                .expect("the lock")
                .challenges
                .remove(digest.as_bytes());
            Ok(())
        }
    }

    #[derive(Default)]
    struct Issuing(Mutex<u32>);

    impl SessionStore for Issuing {
        async fn issue(
            &self,
            _account: AccountId,
            _org: Option<TenantId>,
            _from: Option<&Agent>,
        ) -> Result<SessionToken, SessionError> {
            *self.0.lock().expect("the lock") += 1;
            Ok(SessionToken::from_plaintext("ct_ss_issued"))
        }

        async fn resolve(&self, _token: &SessionToken) -> Result<Session, SessionError> {
            Ok(Session {
                account: an_account(),
                email: an_address(),
                name: None,
                photo: None,
                org: None,
                expires_at: UnixSeconds::new(NOW + 60),
            })
        }

        async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
            Ok(())
        }
    }

    fn an_account() -> AccountId {
        AccountId::new(uuid::Uuid::from_u128(7))
    }

    fn an_address() -> Email {
        Email::try_from("person@example.com").expect("an address")
    }

    fn a_session() -> SessionToken {
        SessionToken::from_plaintext("ct_ss_live")
    }

    fn a_secret() -> TotpSecret {
        TotpSecret::from_bytes([3_u8; 20])
    }

    fn code_now(secret: &TotpSecret) -> TotpCode {
        secret.code_at(Step::at(UnixSeconds::new(NOW)))
    }

    fn a_wrong_code(secret: &TotpSecret) -> TotpCode {
        let zero = TotpCode::new(0).expect("six digits");
        if zero == code_now(secret) {
            TotpCode::new(1).expect("six digits")
        } else {
            zero
        }
    }

    #[test]
    fn the_recovery_alphabet_spans_what_it_says() {
        assert_eq!(RECOVERY_ALPHABET.len(), usize::from(RECOVERY_SPAN));
        assert!(!RECOVERY_ALPHABET.contains(&b'i'));
        assert!(!RECOVERY_ALPHABET.contains(&b'l'));
        assert!(!RECOVERY_ALPHABET.contains(&b'o'));
        assert!(!RECOVERY_ALPHABET.contains(&b'0'));
        assert!(!RECOVERY_ALPHABET.contains(&b'1'));
    }

    #[test]
    fn a_minted_recovery_code_is_two_groups_of_five() {
        let code = RecoveryCode::random();

        assert!(code.looks_like_one());
        assert_eq!(code.exposed().len(), 11);
        assert_eq!(code.exposed().as_bytes()[5], b'-');
        assert_eq!(RecoveryCode::ten().len(), RECOVERY_CODES);
    }

    #[test]
    fn a_typed_recovery_code_is_read_however_it_is_typed() {
        let minted = RecoveryCode::random();
        let shouted = minted.exposed().to_uppercase();
        let spaced = minted.exposed().replace('-', " ");
        let bare = minted.exposed().replace('-', "");

        assert_eq!(RecoveryCode::from_plaintext(&shouted).hash(), minted.hash());
        assert_eq!(RecoveryCode::from_plaintext(&spaced).hash(), minted.hash());
        assert_eq!(RecoveryCode::from_plaintext(&bare).hash(), minted.hash());
    }

    #[test]
    fn a_recovery_code_of_the_wrong_shape_is_refused_before_any_lookup() {
        assert!(!RecoveryCode::from_plaintext("abc-def").looks_like_one());
        assert!(!RecoveryCode::from_plaintext("").looks_like_one());
        assert!(!RecoveryCode::from_plaintext("abcde-fghjkm").looks_like_one());
    }

    #[test]
    fn a_character_no_code_can_hold_is_dropped_rather_than_refused() {
        let read = RecoveryCode::from_plaintext("abcde-fghijk");

        assert!(read.looks_like_one());
        assert_eq!(read.exposed(), "abcde-fghjk");
        assert_ne!(
            read.hash(),
            RecoveryCode::from_plaintext("abcde-fghjkm").hash()
        );
    }

    #[test]
    fn neither_a_challenge_nor_a_recovery_code_renders_itself() {
        let challenge = ChallengeToken::random();
        let recovery = RecoveryCode::random();

        assert_eq!(format!("{challenge:?}"), "ChallengeToken(redacted)");
        assert_eq!(format!("{recovery:?}"), "RecoveryCode(redacted)");
        assert!(challenge.looks_like_one());
        assert!(!ChallengeToken::from_plaintext("nonsense").looks_like_one());
    }

    #[tokio::test]
    async fn an_account_with_no_factor_stands_with_nothing() {
        let store = Store::default();
        let sessions = Issuing::default();
        let factors = SecondFactors::new(&store, &sessions, At(NOW));

        let standing = factors.standing(&a_session()).await.expect("a standing");

        assert_eq!(
            standing,
            Standing {
                live: Vec::new(),
                recovery_left: 0,
            }
        );
    }

    #[tokio::test]
    async fn an_enrolment_is_not_offered_until_it_is_confirmed() {
        let store = Store::default();
        let sessions = Issuing::default();
        let factors = SecondFactors::new(&store, &sessions, At(NOW));
        let secret = a_secret();

        let enrolment = factors
            .enrol(&a_session(), &secret)
            .await
            .expect("an enrolment");

        assert_eq!(enrolment.key, secret.base32());
        assert!(enrolment.uri.contains("person@example.com"));
        assert_eq!(
            factors
                .standing(&a_session())
                .await
                .expect("a standing")
                .live,
            Vec::new()
        );
        assert!(!store.confirmed());
    }

    #[tokio::test]
    async fn confirming_the_first_factor_answers_ten_recovery_codes() {
        let secret = a_secret();
        let store = Store::with_factor(&secret, false);
        let sessions = Issuing::default();
        let factors = SecondFactors::new(&store, &sessions, At(NOW));

        let confirmed = factors
            .confirm(&a_session(), code_now(&secret), RecoveryCode::ten())
            .await
            .expect("it confirms");

        let Confirmed::First { recovery } = confirmed else {
            panic!("the first factor mints recovery codes");
        };
        assert_eq!(recovery.len(), RECOVERY_CODES);
        assert_eq!(store.recovery_held(), RECOVERY_CODES);
        assert!(store.confirmed());
        assert_eq!(
            factors.standing(&a_session()).await.expect("a standing"),
            Standing {
                live: vec![FactorKind::Authenticator],
                recovery_left: 10,
            }
        );
    }

    #[tokio::test]
    async fn a_wrong_code_confirms_nothing() {
        let secret = a_secret();
        let store = Store::with_factor(&secret, false);
        let sessions = Issuing::default();
        let factors = SecondFactors::new(&store, &sessions, At(NOW));

        let refused = factors
            .confirm(&a_session(), a_wrong_code(&secret), RecoveryCode::ten())
            .await;

        assert_eq!(refused, Err(FactorError::Refused));
        assert!(!store.confirmed());
        assert_eq!(store.recovery_held(), 0);
    }

    #[tokio::test]
    async fn confirming_without_enrolling_names_no_factor() {
        let store = Store::default();
        let sessions = Issuing::default();
        let factors = SecondFactors::new(&store, &sessions, At(NOW));

        let refused = factors
            .confirm(
                &a_session(),
                TotpCode::new(123_456).expect("six digits"),
                RecoveryCode::ten(),
            )
            .await;

        assert_eq!(refused, Err(FactorError::NoFactor));
    }

    #[tokio::test]
    async fn forgetting_the_last_factor_leaves_no_recovery_code() {
        let secret = a_secret();
        let store = Store::with_factor(&secret, true);
        let sessions = Issuing::default();
        let factors = SecondFactors::new(&store, &sessions, At(NOW));
        store.holding(&RecoveryCode::ten());

        factors
            .forget(&a_session(), FactorKind::Authenticator)
            .await
            .expect("it is forgotten");

        assert_eq!(store.recovery_held(), 0);
        assert_eq!(
            factors
                .forget(&a_session(), FactorKind::Authenticator)
                .await,
            Err(FactorError::NoFactor)
        );
    }

    #[tokio::test]
    async fn regenerating_replaces_every_code_and_needs_a_factor() {
        let secret = a_secret();
        let store = Store::with_factor(&secret, true);
        let sessions = Issuing::default();
        let factors = SecondFactors::new(&store, &sessions, At(NOW));
        let first = RecoveryCode::ten();
        store.holding(&first);

        let second = factors
            .regenerate(&a_session(), RecoveryCode::ten())
            .await
            .expect("fresh codes");

        assert_eq!(second.len(), RECOVERY_CODES);
        assert_eq!(store.recovery_held(), RECOVERY_CODES);
        assert!(
            !store.still_holds(&first[0]),
            "a code from the old set opens nothing"
        );
    }

    #[tokio::test]
    async fn regenerating_without_a_factor_is_refused() {
        let store = Store::default();
        let sessions = Issuing::default();
        let factors = SecondFactors::new(&store, &sessions, At(NOW));

        let refused = factors.regenerate(&a_session(), RecoveryCode::ten()).await;

        assert_eq!(refused, Err(FactorError::NoFactor));
    }

    #[tokio::test]
    async fn the_code_of_the_moment_spends_the_challenge_and_opens_a_session() {
        let secret = a_secret();
        let store = Store::with_factor(&secret, true);
        let sessions = Issuing::default();
        let step = SecondStep::new(&store, &sessions, At(NOW));
        let challenge = ChallengeToken::random();
        store.challenging(challenge.hash());

        let token = step
            .verify(&challenge, &Answer::Code(code_now(&secret)), None)
            .await
            .expect("a session");

        assert_eq!(token.exposed(), "ct_ss_issued");
        assert_eq!(store.live_challenges(), 0);
    }

    #[tokio::test]
    async fn a_code_already_spent_never_opens_a_second_challenge() {
        let secret = a_secret();
        let store = Store::with_factor(&secret, true);
        let sessions = Issuing::default();
        let step = SecondStep::new(&store, &sessions, At(NOW));
        let once = ChallengeToken::random();
        let twice = ChallengeToken::random();
        store.challenging(once.hash());
        store.challenging(twice.hash());

        let code = code_now(&secret);
        step.verify(&once, &Answer::Code(code), None)
            .await
            .expect("a session");
        let refused = step.verify(&twice, &Answer::Code(code), None).await;

        assert_eq!(refused, Err(FactorError::Refused));
    }

    #[tokio::test]
    async fn an_unconfirmed_factor_answers_no_challenge() {
        let secret = a_secret();
        let store = Store::with_factor(&secret, false);
        let sessions = Issuing::default();
        let step = SecondStep::new(&store, &sessions, At(NOW));
        let challenge = ChallengeToken::random();
        store.challenging(challenge.hash());

        let refused = step
            .verify(&challenge, &Answer::Code(code_now(&secret)), None)
            .await;

        assert_eq!(refused, Err(FactorError::Refused));
    }

    #[tokio::test]
    async fn a_challenge_nobody_opened_is_unknown() {
        let store = Store::with_factor(&a_secret(), true);
        let sessions = Issuing::default();
        let step = SecondStep::new(&store, &sessions, At(NOW));

        let refused = step
            .verify(
                &ChallengeToken::random(),
                &Answer::Code(TotpCode::new(1).expect("six digits")),
                None,
            )
            .await;

        assert_eq!(refused, Err(FactorError::NoChallenge));
    }

    #[tokio::test]
    async fn the_answer_after_the_last_one_closes_the_challenge() {
        let secret = a_secret();
        let store = Store::with_factor(&secret, true);
        let sessions = Issuing::default();
        let step = SecondStep::new(&store, &sessions, At(NOW));
        let challenge = ChallengeToken::random();
        store.challenging(challenge.hash());
        let wrong = Answer::Code(a_wrong_code(&secret));

        for _ in 0..MAX_ANSWERS {
            assert_eq!(
                step.verify(&challenge, &wrong, None).await,
                Err(FactorError::Refused)
            );
        }
        let exhausted = step
            .verify(&challenge, &Answer::Code(code_now(&secret)), None)
            .await;

        assert_eq!(exhausted, Err(FactorError::Exhausted));
        assert_eq!(store.live_challenges(), 0);
    }

    #[tokio::test]
    async fn a_recovery_code_spends_a_challenge_once() {
        let store = Store::with_factor(&a_secret(), true);
        let sessions = Issuing::default();
        let step = SecondStep::new(&store, &sessions, At(NOW));
        let codes = RecoveryCode::ten();
        store.holding(&codes);
        let once = ChallengeToken::random();
        let twice = ChallengeToken::random();
        store.challenging(once.hash());
        store.challenging(twice.hash());

        let opened = step
            .verify(&once, &Answer::Recovery(codes[0].clone()), None)
            .await
            .expect("a session");
        let refused = step
            .verify(&twice, &Answer::Recovery(codes[0].clone()), None)
            .await;

        assert_eq!(opened.exposed(), "ct_ss_issued");
        assert_eq!(refused, Err(FactorError::Refused));
        assert_eq!(store.recovery_held(), RECOVERY_CODES - 1);
    }

    #[tokio::test]
    async fn a_refused_answer_leaves_the_challenge_open() {
        let secret = a_secret();
        let store = Store::with_factor(&secret, true);
        let sessions = Issuing::default();
        let step = SecondStep::new(&store, &sessions, At(NOW));
        let challenge = ChallengeToken::random();
        store.challenging(challenge.hash());

        let refused = step
            .verify(&challenge, &Answer::Code(a_wrong_code(&secret)), None)
            .await;
        let opened = step
            .verify(&challenge, &Answer::Code(code_now(&secret)), None)
            .await;

        assert_eq!(refused, Err(FactorError::Refused));
        assert!(opened.is_ok(), "the challenge survived a wrong answer");
    }
}
