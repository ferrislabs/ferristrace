use std::collections::HashMap;
use std::sync::Mutex;

use super::entities::Session;
use super::errors::{FactorError, SessionError};
use super::ports::{FactorStore, SessionStore};
use super::second_factors::SecondFactors;
use super::second_step::{MAX_ANSWERS, SecondStep};
use super::value_objects::{
    AccountId, Agent, Answer, ChallengeToken, Confirmed, Email, FactorKind, Kept, Pending,
    RECOVERY_ALPHABET, RECOVERY_CODES, RECOVERY_SPAN, RecoveryCode, Standing, Step, TotpCode,
    TotpSecret,
};
use crate::clock::{Clock, UnixSeconds};
use crate::secret::{SecretHash, SessionToken};
use crate::telemetry::TenantId;

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
        self.0.lock().expect("the lock").recovery = codes.iter().map(RecoveryCode::hash).collect();
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
        Ok(SessionToken::from_plaintext("ft_ss_issued"))
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
    SessionToken::from_plaintext("ft_ss_live")
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

    assert_eq!(token.exposed(), "ft_ss_issued");
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

    assert_eq!(opened.exposed(), "ft_ss_issued");
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
