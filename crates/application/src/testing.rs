use core::future::Future;

use crate::registry::{Settle, UnitOfWork};

pub trait Untransacted: Send + Sync + 'static {}

impl<T: Untransacted> UnitOfWork for T {
    type Tx = ();

    fn transaction<R, E, F, Fut>(
        &self,
        _unavailable: impl Fn() -> E + Send,
        _settle: Settle,
        work: F,
    ) -> impl Future<Output = Result<R, E>> + Send
    where
        R: Send,
        E: Send,
        F: FnOnce(Self::Tx) -> Fut + Send,
        Fut: Future<Output = Result<R, E>> + Send,
    {
        work(())
    }
}
