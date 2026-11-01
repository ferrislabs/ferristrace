use core::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::registry::UnitOfWork;
use crate::{Ferristrace, Stack, repository, transactional};

#[derive(Debug, Default)]
pub struct Ledger {
    commits: AtomicUsize,
    rollbacks: AtomicUsize,
    writes: AtomicUsize,
}

#[derive(Debug, Default, Clone)]
pub struct Recording(Arc<Ledger>);

impl UnitOfWork for Recording {
    type Tx = Arc<AtomicUsize>;

    fn transaction<T, E, F, Fut>(
        &self,
        _unavailable: impl Fn() -> E + Send,
        work: F,
    ) -> impl Future<Output = Result<T, E>> + Send
    where
        T: Send,
        E: Send,
        F: FnOnce(Self::Tx) -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send,
    {
        async move {
            let pending = Arc::new(AtomicUsize::new(0));
            let outcome = work(Arc::clone(&pending)).await;
            if outcome.is_ok() {
                self.0.commits.fetch_add(1, Ordering::SeqCst);
                self.0
                    .writes
                    .fetch_add(pending.load(Ordering::SeqCst), Ordering::SeqCst);
            } else {
                self.0.rollbacks.fetch_add(1, Ordering::SeqCst);
            }
            outcome
        }
    }
}

#[repository(domain = Probe, backend = Recording)]
#[derive(Debug)]
pub struct ProbeRepository {
    pending: Arc<AtomicUsize>,
}

impl ProbeRepository {
    fn new(_backend: &Recording, tx: &Arc<AtomicUsize>) -> Self {
        Self {
            pending: Arc::clone(tx),
        }
    }

    fn write(&self) {
        self.pending.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct Probing;

impl Stack for Probing {
    type Repos = Recording;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refused {
    Unavailable,
    Halfway,
}

impl<S: Stack<Repos = Recording>> Ferristrace<S> {
    #[transactional(probe, unavailable = Refused::Unavailable)]
    async fn write_twice(&self, stop_halfway: bool) -> Result<usize, Refused> {
        probe.write();
        if stop_halfway {
            return Err(Refused::Halfway);
        }
        probe.write();
        Ok(2)
    }
}

#[tokio::test]
async fn a_finished_use_case_commits_what_it_wrote() {
    let repos = Recording::default();
    let app = Ferristrace::<Probing>::new(repos.clone());

    assert_eq!(app.write_twice(false).await, Ok(2));

    assert_eq!(repos.0.commits.load(Ordering::SeqCst), 1);
    assert_eq!(repos.0.writes.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_refused_use_case_keeps_nothing() {
    let repos = Recording::default();
    let app = Ferristrace::<Probing>::new(repos.clone());

    assert_eq!(app.write_twice(true).await, Err(Refused::Halfway));

    assert_eq!(repos.0.rollbacks.load(Ordering::SeqCst), 1);
    assert_eq!(repos.0.writes.load(Ordering::SeqCst), 0);
}
