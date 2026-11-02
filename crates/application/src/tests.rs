use std::sync::atomic::{AtomicUsize, Ordering};

use ferristrace_domain::{
    Action, Actor, Bootstrapped, ExportError, Fingerprint, IssueError, KeyError, KeyName,
    KeyScopes, ProjectId, ProvisionError, SettingError, Status, Target,
};

use crate::IssueRuling;
use crate::fakes::{Probing, a_name, a_token, the_account, the_email, the_org, the_policy};
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

#[tokio::test]
async fn issuing_a_key_builds_every_repository_from_one_transaction_and_commits_once() {
    let repos = Recording::default();

    let issued = app(&repos)
        .issue_key(
            &a_token(),
            the_org(),
            None,
            &KeyName::try_from("ci").expect("a name"),
            &KeyScopes::ingest(),
        )
        .await
        .expect("an owner issues a key");

    assert_eq!(&*issued.key.name, "ci");
    assert_eq!(count(&repos.0.opened), 1);
    assert_eq!(repos.0.built_in(), vec![0, 0, 0, 0]);
    assert_eq!(count(&repos.0.commits), 1);
    assert_eq!(count(&repos.0.writes), 1);
}

#[tokio::test]
async fn revoking_a_key_nobody_holds_rolls_back() {
    let repos = Recording::default();

    let refused = app(&repos)
        .revoke_key(
            &a_token(),
            the_org(),
            ferristrace_domain::KeyId::new(uuid::Uuid::nil()),
        )
        .await;

    assert_eq!(refused, Err(KeyError::NoSuchKey));
    assert_eq!(count(&repos.0.rollbacks), 1);
    assert_eq!(count(&repos.0.writes), 0);
}

#[tokio::test]
async fn renaming_an_org_commits_exactly_one_entry_naming_the_account() {
    let repos = Recording::default();
    *repos.0.org_name.lock().expect("the ledger lock") = "Acme".to_owned();

    app(&repos)
        .rename_org(&a_token(), the_org(), &a_name("Acme Inc"))
        .await
        .expect("a renamed organisation");

    let committed = repos.0.committed();
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].action, Action::OrgRenamed);
    assert_eq!(committed[0].actor, Actor::Account(the_account()));
    assert_eq!(committed[0].target, Target::org(the_org()));
    assert_eq!(committed[0].detail.get("from"), Some(Some("Acme")));
    assert_eq!(committed[0].detail.get("to"), Some(Some("Acme Inc")));
}

#[tokio::test]
async fn a_refused_rename_rolls_back_and_commits_no_entry() {
    let repos = Recording::default();
    repos.0.member_only.store(true, Ordering::SeqCst);

    let refused = app(&repos)
        .rename_org(&a_token(), the_org(), &a_name("Mine Now"))
        .await;

    assert_eq!(refused.err(), Some(SettingError::NotYoursToChange));
    assert_eq!(count(&repos.0.rollbacks), 1);
    assert!(repos.0.committed().is_empty());
}

#[tokio::test]
async fn a_ruling_refused_after_its_decision_was_noted_commits_no_entry() {
    let repos = Recording::default();
    repos.0.refuse_assignments.store(true, Ordering::SeqCst);
    let ruled = Ferristrace::<Probing>::new(
        repos.clone(),
        Adapters {
            issue_store: Some(Inert::default()),
            ..adapters()
        },
    );

    let refused = ruled
        .rule_on_issue(
            &a_token(),
            the_org(),
            IssueRuling {
                project: ProjectId::new(uuid::Uuid::from_u128(3)),
                print: Fingerprint::new([1; 16]),
                status: Some(Status::Resolved),
                owner: Some(None),
            },
        )
        .await;

    assert_eq!(refused, Err(IssueError::Unavailable));
    assert_eq!(repos.0.noted.load(Ordering::SeqCst), 1);
    assert_eq!(count(&repos.0.rollbacks), 1);
    assert!(repos.0.committed().is_empty());
}

#[tokio::test]
async fn a_command_line_bootstrap_commits_one_founding_by_the_operator() {
    let repos = Recording::default();

    app(&repos)
        .bootstrap(&a_name("Acme"), &the_email(), false)
        .await
        .expect("a founded organisation");

    let committed = repos.0.committed();
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].action, Action::OrgFounded);
    assert_eq!(committed[0].actor, Actor::Operator);
}

fn exporting_app(repos: &Recording) -> Ferristrace<Probing> {
    Ferristrace::new(
        repos.clone(),
        Adapters {
            span_store: Some(Inert::default()),
            ..adapters()
        },
    )
}

#[tokio::test]
async fn an_export_audits_the_owner_in_the_transaction_that_reads_the_snapshot() {
    let repos = Recording::default();

    let prepared = exporting_app(&repos)
        .begin_export(&a_token(), the_org())
        .await
        .expect("an owner exports");

    assert_eq!(prepared.snapshot.org.id, the_org());
    assert_eq!(count(&repos.0.opened), 1);
    assert_eq!(count(&repos.0.commits), 1);
    let committed = repos.0.committed();
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].action, Action::OrgExported);
    assert_eq!(committed[0].actor, Actor::Account(the_account()));
}

#[tokio::test]
async fn a_refused_export_commits_no_audit_entry() {
    let repos = Recording::default();
    repos.0.member_only.store(true, Ordering::SeqCst);

    let refused = exporting_app(&repos)
        .begin_export(&a_token(), the_org())
        .await;

    assert_eq!(refused.err(), Some(ExportError::OwnersOnly));
    assert_eq!(count(&repos.0.rollbacks), 1);
    assert!(repos.0.committed().is_empty());
}

#[tokio::test]
async fn without_a_span_store_there_is_nothing_to_export_and_nothing_is_audited() {
    let repos = Recording::default();

    let refused = app(&repos).begin_export(&a_token(), the_org()).await;

    assert_eq!(refused.err(), Some(ExportError::StoreUnreachable));
    assert!(repos.0.committed().is_empty());
}
