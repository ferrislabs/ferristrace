use core::future::Future;

use ferristrace_domain::{
    AccountId, Email, HashError, LiveSession, Mailer, Password, PasswordCheck, PasswordHash,
    PasswordHasher, ResetError, ResetToken, SessionError, SessionId, SessionRoll, SessionToken,
};

pub(crate) struct Borrowed<'a, T>(pub(crate) &'a T);

impl<T: Mailer + Sync> Mailer for Borrowed<'_, T> {
    fn send_password_reset(
        &self,
        to: &Email,
        token: &ResetToken,
    ) -> impl Future<Output = Result<(), ResetError>> + Send {
        self.0.send_password_reset(to, token)
    }
}

impl<T: PasswordHasher + Sync> PasswordHasher for Borrowed<'_, T> {
    fn hash(
        &self,
        password: &Password,
    ) -> impl Future<Output = Result<PasswordHash, HashError>> + Send {
        self.0.hash(password)
    }

    fn check(
        &self,
        password: &Password,
        against: &PasswordHash,
    ) -> impl Future<Output = Result<PasswordCheck, HashError>> + Send {
        self.0.check(password, against)
    }

    fn decoy(&self) -> &PasswordHash {
        self.0.decoy()
    }
}

impl<T: SessionRoll + Sync> SessionRoll for Borrowed<'_, T> {
    fn live(
        &self,
        account: AccountId,
        current: &SessionToken,
    ) -> impl Future<Output = Result<Vec<LiveSession>, SessionError>> + Send {
        self.0.live(account, current)
    }

    fn revoke_one(
        &self,
        account: AccountId,
        id: SessionId,
    ) -> impl Future<Output = Result<bool, SessionError>> + Send {
        self.0.revoke_one(account, id)
    }

    fn revoke_others(
        &self,
        account: AccountId,
        current: &SessionToken,
    ) -> impl Future<Output = Result<u64, SessionError>> + Send {
        self.0.revoke_others(account, current)
    }
}
