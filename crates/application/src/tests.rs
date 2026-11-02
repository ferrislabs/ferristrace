use std::sync::atomic::{AtomicUsize, Ordering};

use ferristrace_domain::{Bootstrapped, ProvisionError};

use crate::fakes::{Probing, a_name, a_token, the_email, the_org, the_policy};
use crate::recording::{Handle, Recording};
use crate::testing::Inert;
use crate::{Adapters, Ferristrace, Stack, repository, transactional};

fn adapters() -> Adapters<Probing> {
    Adapters {
        clock: Inert::default(),
        hasher: Inert::default(),
        mailer: Inert::default(),
        photos: ferristrace_domain::NoPhotos,
        meter: Inert::default(),
        span_store: None,
        issue_store: None,
        oidc: None,
        provision: the_policy(),
    }
}

fn app(repos: &Recording) -> Ferristrace<Probing> {
    Ferristrace::new(repos.clone(), adapters())
}

#[repository(domain = Probe, backend = Recording)]
#[derive(Debug)]
pub struct ProbeRepository {
    handle: Handle,
}

impl ProbeRepository {
    fn new(_backend: &Recording, tx: &Handle) -> Self {
        Self { handle: tx.clone() }
    }

    fn write(&self) {
        self.handle.write();
    }
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

    #[transactional(probe, unavailable = Refused::Unavailable, settle = always)]
    async fn refuse_after_counting(&self) -> Result<usize, Refused> {
        probe.write();
        Err(Refused::Halfway)
    }
}

fn count(of: &AtomicUsize) -> usize {
    of.load(Ordering::SeqCst)
}

#[tokio::test]
async fn a_finished_use_case_commits_what_it_wrote() {
    let repos = Recording::default();

    assert_eq!(app(&repos).write_twice(false).await, Ok(2));

    assert_eq!(count(&repos.0.commits), 1);
    assert_eq!(count(&repos.0.writes), 2);
}

#[tokio::test]
async fn a_use_case_settled_always_keeps_what_it_wrote_before_refusing() {
    let repos = Recording::default();

    assert_eq!(
        app(&repos).refuse_after_counting().await,
        Err(Refused::Halfway)
    );

    assert_eq!(count(&repos.0.commits), 1);
    assert_eq!(count(&repos.0.rollbacks), 0);
    assert_eq!(count(&repos.0.writes), 1);
}

#[tokio::test]
async fn a_refused_use_case_keeps_nothing() {
    let repos = Recording::default();

    assert_eq!(app(&repos).write_twice(true).await, Err(Refused::Halfway));

    assert_eq!(count(&repos.0.rollbacks), 1);
    assert_eq!(count(&repos.0.writes), 0);
}

#[tokio::test]
async fn renaming_an_org_builds_every_repository_from_one_transaction_and_commits_once() {
    let repos = Recording::default();

    let card = app(&repos)
        .rename_org(&a_token(), the_org(), &a_name("Acme Inc"))
        .await
        .expect("a renamed organisation");

    assert_eq!(&*card.name, "Acme Inc");
    assert_eq!(count(&repos.0.opened), 1);
    assert_eq!(repos.0.built_in(), vec![0, 0, 0]);
    assert_eq!(count(&repos.0.commits), 1);
    assert_eq!(count(&repos.0.rollbacks), 0);
    assert_eq!(count(&repos.0.writes), 1);
}

#[tokio::test]
async fn bootstrapping_runs_in_one_transaction_and_commits_once() {
    let repos = Recording::default();

    let outcome = app(&repos)
        .bootstrap(&a_name("Acme"), &the_email(), false)
        .await
        .expect("a founded organisation");

    assert!(matches!(outcome, Bootstrapped::Founded(_)));
    assert_eq!(count(&repos.0.opened), 1);
    assert_eq!(repos.0.built_in(), vec![0]);
    assert_eq!(count(&repos.0.commits), 1);
    assert_eq!(count(&repos.0.writes), 2);
}

#[tokio::test]
async fn bootstrapping_interrupted_at_the_project_rolls_the_organisation_back() {
    let repos = Recording::default();
    repos.0.refuse_claims.store(true, Ordering::SeqCst);

    let outcome = app(&repos)
        .bootstrap(&a_name("Acme"), &the_email(), false)
        .await;

    assert_eq!(outcome, Err(ProvisionError::Unreachable));
    assert_eq!(count(&repos.0.opened), 1);
    assert_eq!(count(&repos.0.rollbacks), 1);
    assert_eq!(count(&repos.0.commits), 0);
    assert_eq!(count(&repos.0.writes), 0);
    assert!(count(&repos.0.attempts) >= 3);
}
