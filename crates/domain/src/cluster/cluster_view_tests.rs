use std::sync::Mutex;

use super::cluster_view::{ClusterView, fresh_window};
use super::entities::{ClusterSnapshot, Pod};
use super::ports::ClusterReader;
use super::value_objects::{ClusterFilter, PodPhase, PodReason};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OTHER, OURS, OneSession, Ring, THEIRS, a_project, an_org, scopes, token,
};
use crate::access::{KeyScope, KeyScopes, Reader, Readers};
use crate::catalogue::ProjectId;
use crate::clock::UnixSeconds;
use crate::explore::{ExploreError, Window};
use crate::organisation::{OrgRole, Whoami};
use crate::placement::PlacementName;
use crate::store::StoreError;
use crate::telemetry::TenantId;

const NOW: u64 = 1_700_000_000;
const SECOND: u64 = 1_000_000_000;

type Asked = (TenantId, Window, Option<ProjectId>, Option<PlacementName>);

#[derive(Default)]
struct Shelf {
    asked: Mutex<Vec<Asked>>,
}

impl Shelf {
    fn asked(&self) -> Vec<Asked> {
        self.asked.lock().expect("the lock").clone()
    }
}

impl ClusterReader for &Shelf {
    async fn snapshot(
        &self,
        org: TenantId,
        window: Window,
        filter: &ClusterFilter,
    ) -> Result<ClusterSnapshot, StoreError> {
        self.asked.lock().expect("the lock").push((
            org,
            window,
            filter.project,
            filter.cluster.clone(),
        ));
        Ok(ClusterSnapshot {
            pods: vec![Pod {
                phase: Some(PodPhase::Running),
                restarts_in_window: 4,
                ..Pod::named("mailer-1", "shop")
            }],
            ..ClusterSnapshot::default()
        })
    }
}

struct Unreachable;

impl ClusterReader for Unreachable {
    async fn snapshot(
        &self,
        _org: TenantId,
        _window: Window,
        _filter: &ClusterFilter,
    ) -> Result<ClusterSnapshot, StoreError> {
        Err(StoreError::Unreachable)
    }
}

fn viewing<R: ClusterReader>(
    keys: &Ring,
    reader: R,
) -> ClusterView<OneSession, MemberOfMine, &Ring, R> {
    ClusterView::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine(OrgRole::Member)), keys),
        reader,
    )
}

fn reading() -> KeyScopes {
    scopes(&[KeyScope::Read])
}

#[tokio::test]
async fn a_member_sees_the_last_ten_minutes_of_its_cluster_judged() {
    let keys = Ring::default();
    let shelf = Shelf::default();
    let filter = ClusterFilter {
        cluster: Some(PlacementName::try_from("prod").expect("a cluster")),
        ..ClusterFilter::default()
    };

    let seen = viewing(&keys, &shelf)
        .snapshot(
            &Reader::Session(token()),
            an_org(MINE),
            UnixSeconds::new(NOW),
            &filter,
        )
        .await
        .expect("a member may look");

    assert_eq!(seen.pods[0].reason, Some(PodReason::Restarting));
    assert!(seen.pods[0].unhealthy);
    let asked = shelf.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, an_org(MINE));
    assert_eq!(asked[0].1, fresh_window(UnixSeconds::new(NOW)));
    assert_eq!(asked[0].1.to_unix_nanos(), (NOW + 1) * SECOND);
    assert_eq!(asked[0].1.from_unix_nanos(), (NOW - 600) * SECOND);
    assert_eq!(asked[0].3, filter.cluster);
}

#[tokio::test]
async fn a_stranger_is_refused_before_the_store_is_asked() {
    let keys = Ring::default();
    let shelf = Shelf::default();

    let refused = viewing(&keys, &shelf)
        .snapshot(
            &Reader::Session(token()),
            an_org(THEIRS),
            UnixSeconds::new(NOW),
            &ClusterFilter::default(),
        )
        .await;

    assert_eq!(refused, Err(ExploreError::NotYours));
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn a_project_bound_read_key_sees_only_its_project() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), Some(a_project(OURS)), reading());
    let shelf = Shelf::default();
    let view = viewing(&keys, &shelf);

    view.snapshot(
        &Reader::Key(secret.clone()),
        an_org(MINE),
        UnixSeconds::new(NOW),
        &ClusterFilter::default(),
    )
    .await
    .expect("its own project");
    let elsewhere = view
        .snapshot(
            &Reader::Key(secret),
            an_org(MINE),
            UnixSeconds::new(NOW),
            &ClusterFilter {
                project: Some(a_project(OTHER)),
                ..ClusterFilter::default()
            },
        )
        .await;

    assert_eq!(elsewhere, Err(ExploreError::NotYours));
    assert_eq!(
        shelf
            .asked()
            .into_iter()
            .map(|asked| asked.2)
            .collect::<Vec<_>>(),
        vec![Some(a_project(OURS))]
    );
}

#[tokio::test]
async fn an_ingest_key_sees_no_cluster() {
    let keys = Ring::default();
    let secret = keys.holding(an_org(MINE), None, KeyScopes::ingest());
    let shelf = Shelf::default();

    let refused = viewing(&keys, &shelf)
        .snapshot(
            &Reader::Key(secret),
            an_org(MINE),
            UnixSeconds::new(NOW),
            &ClusterFilter::default(),
        )
        .await;

    assert_eq!(refused, Err(ExploreError::NoSession));
    assert!(shelf.asked().is_empty());
}

#[tokio::test]
async fn an_unreachable_store_is_reported_as_such() {
    let keys = Ring::default();

    let refused = viewing(&keys, Unreachable)
        .snapshot(
            &Reader::Session(token()),
            an_org(MINE),
            UnixSeconds::new(NOW),
            &ClusterFilter::default(),
        )
        .await;

    assert_eq!(refused, Err(ExploreError::StoreUnreachable));
}

#[test]
fn a_fresh_window_at_the_epoch_does_not_wrap() {
    let window = fresh_window(UnixSeconds::new(0));

    assert_eq!(window.from_unix_nanos(), 0);
    assert_eq!(window.to_unix_nanos(), SECOND);
}
