use core::future::Future;

pub trait UnitOfWork: Send + Sync + 'static {
    type Tx: Clone + Send + Sync + 'static;

    fn transaction<T, E, F, Fut>(
        &self,
        unavailable: impl Fn() -> E + Send,
        work: F,
    ) -> impl Future<Output = Result<T, E>> + Send
    where
        T: Send,
        E: Send,
        F: FnOnce(Self::Tx) -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send;
}

pub trait RepoFor<D>: UnitOfWork {
    type Repo: Send + Sync;

    fn build(&self, tx: &Self::Tx) -> Self::Repo;
}

pub mod domain {
    #[cfg(test)]
    #[derive(Debug)]
    pub struct Probe;
}

pub trait Repositories: UnitOfWork {}

impl<R> Repositories for R where R: UnitOfWork {}
