use core::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ferristrace_domain::AuditEntry;

use crate::registry::{Settle, UnitOfWork};

#[derive(Debug, Default)]
pub struct Ledger {
    pub opened: AtomicUsize,
    pub commits: AtomicUsize,
    pub rollbacks: AtomicUsize,
    pub writes: AtomicUsize,
    pub attempts: AtomicUsize,
    pub refuse_claims: AtomicBool,
    pub refuse_assignments: AtomicBool,
    pub member_only: AtomicBool,
    pub noted: AtomicUsize,
    pub built_in: Mutex<Vec<usize>>,
    pub org_name: Mutex<String>,
    pub entries: Mutex<Vec<AuditEntry>>,
}

impl Ledger {
    pub fn built_in(&self) -> Vec<usize> {
        self.built_in.lock().expect("the ledger lock").clone()
    }

    pub fn committed(&self) -> Vec<AuditEntry> {
        self.entries.lock().expect("the ledger lock").clone()
    }
}

#[derive(Debug, Default, Clone)]
pub struct Recording(pub Arc<Ledger>);

#[derive(Debug, Clone)]
pub struct Handle {
    pub id: usize,
    pub pending: Arc<AtomicUsize>,
    pub noted: Arc<Mutex<Vec<AuditEntry>>>,
    pub ledger: Arc<Ledger>,
}

impl Handle {
    pub fn note(&self, entry: &AuditEntry) {
        self.ledger.noted.fetch_add(1, Ordering::SeqCst);
        self.noted
            .lock()
            .expect("the ledger lock")
            .push(entry.clone());
    }

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

    async fn transaction<T, E, F, Fut>(
        &self,
        _unavailable: impl Fn() -> E + Send,
        settle: Settle,
        work: F,
    ) -> Result<T, E>
    where
        T: Send,
        E: Send,
        F: FnOnce(Self::Tx) -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send,
    {
        let handle = Handle {
            id: self.0.opened.fetch_add(1, Ordering::SeqCst),
            pending: Arc::new(AtomicUsize::new(0)),
            noted: Arc::default(),
            ledger: Arc::clone(&self.0),
        };
        let pending = Arc::clone(&handle.pending);
        let noted = Arc::clone(&handle.noted);
        let outcome = work(handle).await;
        if outcome.is_ok() || settle == Settle::Always {
            self.0.commits.fetch_add(1, Ordering::SeqCst);
            self.0
                .writes
                .fetch_add(pending.load(Ordering::SeqCst), Ordering::SeqCst);
            self.0
                .entries
                .lock()
                .expect("the ledger lock")
                .append(&mut noted.lock().expect("the ledger lock"));
        } else {
            self.0.rollbacks.fetch_add(1, Ordering::SeqCst);
        }
        outcome
    }
}
