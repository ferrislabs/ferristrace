use std::sync::Mutex;

use super::errors::IssueError;
use super::issues::Issues;
use super::ports::{IssueDecisions, IssueReader};
use super::value_objects::{Counted, Fingerprint, IssueFilter, Occurrence, Recorded, Status};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OURS, OneSession, Ring, THEIRS, a_project, an_org, scopes, token,
};
use crate::access::{KeyScope, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::clock::{Clock, UnixSeconds};
use crate::explore::Window;
use crate::identity::AccountId;
use crate::organisation::{OrgRole, Whoami};
use crate::store::StoreError;
use crate::telemetry::TenantId;

const OTHER: u128 = 40;

#[derive(Default)]
struct Asked {
    projects: Mutex<Vec<Option<ProjectId>>>,
}

impl Asked {
    fn projects(&self) -> Vec<Option<ProjectId>> {
        self.projects.lock().expect("the lock").clone()
    }
}

impl IssueReader for &Asked {
    async fn issues(
        &self,
        _tenant: TenantId,
        filter: &IssueFilter,
    ) -> Result<Vec<Counted>, StoreError> {
        self.projects.lock().expect("the lock").push(filter.project);
        Ok(Vec::new())
    }

    async fn occurrences(
        &self,
        _tenant: TenantId,
        project: ProjectId,
        _print: Fingerprint,
        _window: Window,
        _limit: u32,
    ) -> Result<Vec<Occurrence>, StoreError> {
        self.projects.lock().expect("the lock").push(Some(project));
        Ok(Vec::new())
    }
}

struct NoDecisions;

impl IssueDecisions for NoDecisions {
    async fn read_many(
        &self,
        _of: &[(ProjectId, Fingerprint)],
    ) -> Result<Vec<Recorded>, StoreError> {
        Ok(Vec::new())
    }

    async fn decide(
        &self,
        _project: ProjectId,
        _print: Fingerprint,
        _status: Status,
        _by: AccountId,
        _at_unix_nanos: u64,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    async fn assign(
        &self,
        _project: ProjectId,
        _print: Fingerprint,
        _owner: Option<AccountId>,
        _by: AccountId,
        _at_unix_nanos: u64,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

struct Still;

impl Clock for Still {
    fn now(&self) -> UnixSeconds {
        UnixSeconds::new(1)
    }
}

fn issues<'a>(
    keys: &'a Ring,
    asked: &'a Asked,
) -> Issues<OneSession, MemberOfMine, &'a Ring, &'a Asked, NoDecisions, Still> {
    Issues::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine(OrgRole::Member)), keys),
        asked,
        NoDecisions,
        Still,
    )
}

fn a_window() -> Window {
    Window::new(0, 1000).expect("a window")
}

fn a_print() -> Fingerprint {
    Fingerprint::new([5; 16])
}

fn bound_to_ours(keys: &Ring) -> Reader {
    Reader::Key(keys.holding(
        an_org(MINE),
        Some(a_project(OURS)),
        scopes(&[KeyScope::Read]),
    ))
}

#[tokio::test]
async fn a_project_bound_read_key_lists_only_the_issues_of_its_project() {
    let keys = Ring::default();
    let asked = Asked::default();

    issues(&keys, &asked)
        .list(
            &bound_to_ours(&keys),
            an_org(MINE),
            &IssueFilter::over(a_window()),
        )
        .await
        .expect("a read key may list its project");

    assert_eq!(asked.projects(), vec![Some(a_project(OURS))]);
}

#[tokio::test]
async fn a_project_bound_read_key_naming_another_project_is_refused_like_a_stranger() {
    let keys = Ring::default();
    let asked = Asked::default();
    let reader = bound_to_ours(&keys);
    let mut filter = IssueFilter::over(a_window());
    filter.project = Some(a_project(OTHER));

    let listed = issues(&keys, &asked)
        .list(&reader, an_org(MINE), &filter)
        .await;
    let one = issues(&keys, &asked)
        .one(
            &reader,
            an_org(MINE),
            Some(a_project(OTHER)),
            a_print(),
            a_window(),
        )
        .await;
    let seen = issues(&keys, &asked)
        .occurrences(
            &reader,
            an_org(MINE),
            a_project(OTHER),
            a_print(),
            a_window(),
            10,
        )
        .await;

    assert_eq!(listed, Err(IssueError::NotYours));
    assert_eq!(one, Err(IssueError::NotYours));
    assert_eq!(seen, Err(IssueError::NotYours));
    assert!(asked.projects().is_empty(), "the store is never asked");
}

#[tokio::test]
async fn a_project_bound_read_key_reads_the_occurrences_of_its_project() {
    let keys = Ring::default();
    let asked = Asked::default();

    issues(&keys, &asked)
        .occurrences(
            &bound_to_ours(&keys),
            an_org(MINE),
            a_project(OURS),
            a_print(),
            a_window(),
            10,
        )
        .await
        .expect("its own project");

    assert_eq!(asked.projects(), vec![Some(a_project(OURS))]);
}

#[tokio::test]
async fn a_read_key_reads_no_issue_of_another_organisation() {
    let keys = Ring::default();
    let asked = Asked::default();

    let refused = issues(&keys, &asked)
        .list(
            &bound_to_ours(&keys),
            an_org(THEIRS),
            &IssueFilter::over(a_window()),
        )
        .await;

    assert_eq!(refused, Err(IssueError::NotYours));
    assert!(asked.projects().is_empty());
}

#[tokio::test]
async fn a_member_session_still_reads_and_decides_as_before() {
    let keys = Ring::default();
    let asked = Asked::default();
    let mut filter = IssueFilter::over(a_window());
    filter.project = Some(a_project(OTHER));

    issues(&keys, &asked)
        .list(&Reader::Session(token()), an_org(MINE), &filter)
        .await
        .expect("a member reads any project");
    issues(&keys, &asked)
        .decide(
            &token(),
            an_org(MINE),
            a_project(OTHER),
            a_print(),
            Status::Resolved,
        )
        .await
        .expect("a member decides");

    assert_eq!(asked.projects(), vec![Some(a_project(OTHER))]);
}
