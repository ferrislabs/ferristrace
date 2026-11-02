use core::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::registry::{Settle, UnitOfWork};

#[derive(Debug, Default)]
pub struct Ledger {
    pub opened: AtomicUsize,
    pub commits: AtomicUsize,
    pub rollbacks: AtomicUsize,
    pub writes: AtomicUsize,
    pub attempts: AtomicUsize,
    pub refuse_claims: AtomicBool,
    pub built_in: Mutex<Vec<usize>>,
    pub org_name: Mutex<String>,
}

impl Ledger {
    pub fn built_in(&self) -> Vec<usize> {
        self.built_in.lock().expect("the ledger lock").clone()
    }
}

#[derive(Debug, Default, Clone)]
pub struct Recording(pub Arc<Ledger>);

#[derive(Debug, Clone)]
pub struct Handle {
    pub id: usize,
    pub pending: Arc<AtomicUsize>,
    pub ledger: Arc<Ledger>,
}

impl Handle {
    pub fn note_build(&self) {
        self.ledger
            .built_in
            .lock()
            .expect("the ledger lock")
            .push(self.id);
    }

    pub fn attempt(&self) {
        self.ledger.attempts.fetch_add(1, Ordering::SeqCst);
    }

    pub fn write(&self) {
        self.attempt();
        self.pending.fetch_add(1, Ordering::SeqCst);
    }
}

impl UnitOfWork for Recording {
    type Tx = Handle;

    fn transaction<T, E, F, Fut>(
        &self,
        _unavailable: impl Fn() -> E + Send,
        settle: Settle,
        work: F,
    ) -> impl Future<Output = Result<T, E>> + Send
    where
        T: Send,
        E: Send,
        F: FnOnce(Self::Tx) -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send,
    {
        async move {
            let handle = Handle {
                id: self.0.opened.fetch_add(1, Ordering::SeqCst),
                pending: Arc::new(AtomicUsize::new(0)),
                ledger: Arc::clone(&self.0),
            };
            let pending = Arc::clone(&handle.pending);
            let outcome = work(handle).await;
            if outcome.is_ok() || settle == Settle::Always {
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
