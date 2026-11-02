use core::future::Future;

use ferristrace_domain::{
    AccountId, LiveSession, SessionError, SessionId, SessionRoll, SessionToken,
};

pub(crate) struct Borrowed<'a, T>(pub(crate) &'a T);

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
