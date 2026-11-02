use std::sync::Mutex;

use super::entities::{Deploy, Release, ReleaseDetail, Traffic};
use super::errors::ReleaseError;
use super::ports::ReleaseReader;
use super::release_view::Releases;
use super::value_objects::ReleaseFilter;
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OTHER, OURS, OneSession, Ring, THEIRS, a_project, an_org, scopes, token,
};
use crate::access::{KeyScope, KeyScopes, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::organisation::{OrgRole, Whoami};
use crate::store::StoreError;
use crate::telemetry::TenantId;

type Asked = (TenantId, Option<ProjectId>);

#[derive(Default)]
struct Shelf {
    asked: Mutex<Vec<Asked>>,
    unknown: bool,
}

impl Shelf {
    fn asked(&self) -> Vec<Asked> {
        self.asked.lock().expect("the lock").clone()
    }
}

fn a_release() -> Release {
    Release {
        service: "checkout".into(),
        version: "1.2.3".into(),
        environments: vec!["prod".into()],
        first_seen_unix_nanos: 10,
        last_seen_unix_nanos: 20,
        requests: 100,
        errors: 5,
        new_issues: 1,
    }
}

impl ReleaseReader for &Shelf {
    async fn releases(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Release>, StoreError> {
        self.asked
            .lock()
            .expect("the lock")
            .push((org, filter.project));
        Ok(vec![a_release()])
    }

    async fn release(
        &self,
        org: TenantId,
        _service: &str,
        _version: &str,
        project: Option<ProjectId>,
    ) -> Result<Option<ReleaseDetail>, StoreError> {
        self.asked.lock().expect("the lock").push((org, project));
        Ok((!self.unknown).then(|| ReleaseDetail {
            release: a_release(),
            before: Traffic::default(),
            after: Traffic::default(),
            introduced: Vec::new(),
        }))
    }

    async fn deploys(
        &self,
        org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Deploy>, StoreError> {
        self.asked
            .lock()
            .expect("the lock")
            .push((org, filter.project));
        Ok(Vec::new())
    }
}

struct Unreachable;

impl ReleaseReader for Unreachable {
    async fn releases(
        &self,
        _org: TenantId,
        _filter: &ReleaseFilter,
    ) -> Result<Vec<Release>, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn release(
        &self,
        _org: TenantId,
        _service: &str,
        _version: &str,
        _project: Option<ProjectId>,
    ) -> Result<Option<ReleaseDetail>, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn deploys(
        &self,
        _org: TenantId,
        _filter: &ReleaseFilter,
    ) -> Result<Vec<Deploy>, StoreError> {
        Err(StoreError::Unreachable)
    }
}

fn over() -> ReleaseFilter {
    ReleaseFilter::over(Window::new(1, 2).expect("a window"))
}

fn releasing<R: ReleaseReader>(
    keys: &Ring,
    reader: R,
) -> Releases<OneSession, MemberOfMine, &Ring, R> {
    Releases::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine(OrgRole::Member)), keys),
        reader,
    )
}

fn reading() -> KeyScopes {
    scopes(&[KeyScope::Read])
}

#[tokio::test]
async fn a_member_lists_the_releases_deploys_and_detail_of_its_org() {
    let keys = Ring::default();
    let shelf = Shelf::default();
    let releases = releasing(&keys, &shelf);
    let who = Reader::Session(token());

    let listed = releases.list(&who, an_org(MINE), &over()).await;
    let deploys = releases.deploys(&who, an_org(MINE), &over()).await;
    let detail = releases
        .detail(&who, an_org(MINE), "checkout", "1.2.3", None)
        .await;

    assert_eq!(listed, Ok(vec![a_release()]));
    assert_eq!(deploys, Ok(Vec::new()));
    assert_eq!(detail.map(|held| held.release), Ok(a_release()));
    assert_eq!(shelf.asked(), vec![(an_org(MINE), None); 3]);
}

#[tokio::test]
async fn a_stranger_is_refused_before_the_store_is_asked() {
    let keys = Ring::default();
    let shelf = Shelf::default();
    let releases = releasing(&keys, &shelf);
    let who = Reader::Session(token());

    assert_eq!(
        releases.list(&who, an_org(THEIRS), &over()).await,
        Err(ReleaseError::NotYours)
    );
    assert_eq!(
        releases.deploys(&who, an_org(THEIRS), &over()).await,
        Err(ReleaseError::NotYours)
    );
    assert_eq!(
        releases
            .detail(&who, an_org(THEIRS), "checkout", "1.2.3", None)
            .await
            .map(|held| held.release),
        Err(ReleaseError::NotYours)
    );
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn a_project_bound_read_key_is_narrowed_and_cannot_ask_for_another_project() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let shelf = Shelf::default();
    let releases = releasing(&keys, &shelf);
    let who = Reader::Key(secret);
    let elsewhere = ReleaseFilter {
        project: Some(a_project(OTHER)),
        ..over()
    };

    releases
        .list(&who, an_org(MINE), &over())
        .await
        .expect("its own project");
    releases
        .deploys(&who, an_org(MINE), &over())
        .await
        .expect("its own project");
    releases
        .detail(&who, an_org(MINE), "checkout", "1.2.3", None)
        .await
        .expect("its own project");

    assert_eq!(
        releases.list(&who, an_org(MINE), &elsewhere).await,
        Err(ReleaseError::NotYours)
    );
    assert_eq!(
        releases.deploys(&who, an_org(MINE), &elsewhere).await,
        Err(ReleaseError::NotYours)
    );
    assert_eq!(
        releases
            .detail(
                &who,
                an_org(MINE),
                "checkout",
                "1.2.3",
                Some(a_project(OTHER))
            )
            .await
            .map(|held| held.release),
        Err(ReleaseError::NotYours)
    );
    assert_eq!(
        shelf.asked(),
        vec![(an_org(MINE), Some(a_project(OURS))); 3]
    );
}

#[tokio::test]
async fn an_ingest_key_reads_no_release() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, KeyScopes::ingest());
    let shelf = Shelf::default();

    let refused = releasing(&keys, &shelf)
        .list(&Reader::Key(secret), an_org(MINE), &over())
        .await;

    assert_eq!(refused, Err(ReleaseError::NoSession));
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn a_release_the_org_never_saw_is_unknown() {
    let keys = Ring::default();
    let shelf = Shelf {
        unknown: true,
        ..Shelf::default()
    };

    let refused = releasing(&keys, &shelf)
        .detail(
            &Reader::Session(token()),
            an_org(MINE),
            "checkout",
            "9",
            None,
        )
        .await
        .map(|held| held.release);

    assert_eq!(refused, Err(ReleaseError::Unknown));
}

#[tokio::test]
async fn an_unreachable_store_is_reported_as_such() {
    let keys = Ring::default();

    let refused = releasing(&keys, Unreachable)
        .list(&Reader::Session(token()), an_org(MINE), &over())
        .await;

    assert_eq!(refused, Err(ReleaseError::StoreUnreachable));
}
