use argon2::password_hash::Error as HashingError;
use argon2::password_hash::phc::PasswordHash as EncodedHash;
use argon2::{Algorithm, Argon2, Params, PasswordHasher as _, PasswordVerifier as _, Version};
use ferristrace_domain::{HashError, Password, PasswordCheck, PasswordHash, PasswordHasher};
use tokio::task;

const OWASP_MEMORY_KIB: u32 = 19 * 1024;
const OWASP_PASSES: u32 = 2;
const OWASP_LANES: u32 = 1;
const DIGEST_BYTES: usize = 32;
const DECOY_BYTES: usize = 32;

#[derive(Debug, Clone)]
pub struct Argon2Hasher {
    context: Argon2<'static>,
    decoy: PasswordHash,
}

impl Argon2Hasher {
    pub fn new() -> Result<Self, HashError> {
        Self::with_cost(OWASP_MEMORY_KIB, OWASP_PASSES, OWASP_LANES)
    }

    pub fn with_cost(memory_kib: u32, passes: u32, lanes: u32) -> Result<Self, HashError> {
        let params = Params::new(memory_kib, passes, lanes, Some(DIGEST_BYTES))
            .map_err(|_| HashError::Unhashable)?;
        let context = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let decoy = unreachable(&context)?;
        Ok(Self { context, decoy })
    }

    pub fn locked(&self) -> Result<PasswordHash, HashError> {
        unreachable(&self.context)
    }
}

impl PasswordHasher for Argon2Hasher {
    async fn hash(&self, password: &Password) -> Result<PasswordHash, HashError> {
        let context = self.context.clone();
        let plaintext = password.exposed().as_bytes().to_vec();
        task::spawn_blocking(move || encode(&context, &plaintext))
            .await
            .map_err(|_| HashError::Unhashable)?
    }

    async fn check(
        &self,
        password: &Password,
        against: &PasswordHash,
    ) -> Result<PasswordCheck, HashError> {
        let context = self.context.clone();
        let plaintext = password.exposed().as_bytes().to_vec();
        let stored = against.encoded().to_owned();
        task::spawn_blocking(move || compare(&context, &plaintext, &stored))
            .await
            .map_err(|_| HashError::Unhashable)?
    }

    fn decoy(&self) -> &PasswordHash {
        &self.decoy
    }
}

fn unreachable(context: &Argon2<'static>) -> Result<PasswordHash, HashError> {
    let bytes: [u8; DECOY_BYTES] = rand::random();
    encode(context, &bytes)
}

fn encode(context: &Argon2<'static>, plaintext: &[u8]) -> Result<PasswordHash, HashError> {
    context
        .hash_password(plaintext)
        .map(|encoded| PasswordHash::from_encoded(&encoded.to_string()))
        .map_err(|_| HashError::Unhashable)
}

fn compare(
    context: &Argon2<'static>,
    plaintext: &[u8],
    stored: &str,
) -> Result<PasswordCheck, HashError> {
    let parsed = EncodedHash::new(stored).map_err(|_| HashError::Unreadable)?;
    if parsed.salt.is_none() || parsed.hash.is_none() {
        return Err(HashError::Unreadable);
    }
    match context.verify_password(plaintext, &parsed) {
        Ok(()) => {}
        Err(HashingError::PasswordInvalid) => return Ok(PasswordCheck::Rejected),
        Err(_) => return Err(HashError::Unreadable),
    }
    if behind(context, &parsed) {
        return Ok(PasswordCheck::AcceptedBehindCost(encode(
            context, plaintext,
        )?));
    }
    Ok(PasswordCheck::Accepted)
}

fn behind(context: &Argon2<'static>, parsed: &EncodedHash) -> bool {
    let Ok(stored) = Params::try_from(parsed) else {
        return true;
    };
    let current = context.params();
    stored.m_cost() < current.m_cost()
        || stored.t_cost() < current.t_cost()
        || stored.p_cost() < current.p_cost()
}
